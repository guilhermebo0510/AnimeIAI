//! Animação (原画 + 動画): mesa de luz (papel cebola), papel do cel (chave, breakdown, intercalação) com a navegação
//! entre chaves, fase do corte (genga/douga) com a trava dos cels-chave no douga e a densidade da linha do tempo.
//!
//! Mesa de luz: os desenhos vizinhos do atual (por quadros, por desenhos ou só as chaves) da camada ativa ou de todas as
//! camadas de animação aparecem tingidos (anterior vermelho, seguinte verde, configuráveis) com opacidade que cai com a
//! distância. Uma thread renderiza cada vizinho sozinho UMA vez (cache por camada, cel, revisão do cel e posição da
//! camada: desenhar no cel atual não refaz os vizinhos), compõe tudo numa textura só e tira dela o que o desenho atual
//! cobre, de modo que os vizinhos ficam por baixo das linhas da camada ativa. É só visão: a textura é desenhada pelo
//! canvas por cima do quadro e nunca entra no render, na exportação nem no documento. Durante a reprodução fica
//! escondida.

use super::commands::{self as cmds, NodeId};
use super::editor::Editor;
use super::panels::timeline::exposure_runs;
use super::render_service::{Snapshot, pick_reduce};
use super::ui_kit::Env;
use crate::i18n::tr;
use crate::layout::LayoutVariant;
use crate::workspace::WorkspaceId;
use aiai_core::*;
use egui::{Color32, ColorImage, Pos2, TextureHandle, TextureOptions};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Condvar, Mutex};

// ------------------------------------------------------------------------------------------------ estado

/// Quais vizinhos a mesa de luz mostra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnionMode {
    /// O que está exposto nos quadros vizinhos (quadro − k, quadro + k), como o papel cebola do Clip Studio.
    Frames,
    /// Os desenhos diferentes anteriores e seguintes (blocos de exposição), como o OpenToonz.
    Drawings,
    /// Só os desenhos-chave vizinhos (genga).
    Keys,
}

/// De quais camadas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnionLayers {
    Active,
    /// Todas as camadas de animação visíveis (vetor e raster).
    Animation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OnionOpts {
    pub on: bool,
    pub before: u32,
    pub after: u32,
    pub mode: OnionMode,
    /// Opacidade do vizinho mais perto (os mais longe caem até 1/n dela).
    pub opacity: f32,
    pub prev: Color32,
    pub next: Color32,
    pub layers: OnionLayers,
}

/// Até quantos vizinhos de cada lado.
pub(crate) const ONION_MAX: u32 = 5;
pub(crate) const ONION_PREV: Color32 = Color32::from_rgb(222, 58, 48);
pub(crate) const ONION_NEXT: Color32 = Color32::from_rgb(34, 160, 72);

impl Default for OnionOpts {
    fn default() -> Self {
        OnionOpts { on: false, before: 2, after: 2, mode: OnionMode::Drawings, opacity: 0.5, prev: ONION_PREV, next: ONION_NEXT, layers: OnionLayers::Active }
    }
}

/// Densidade da linha do tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Density {
    Normal,
    /// Linhas de 14 px, cabeçalho estreito: 12 camadas cabem em ~20 % de uma tela de 1080 px.
    Compact,
}

/// Medidas da linha do tempo desenhada por último (a bridge informa as reais).
#[derive(Debug, Clone, Copy)]
pub(crate) struct TlMetrics {
    pub header_w: f32,
    pub ruler_h: f32,
    pub row_h: f32,
}

pub(crate) struct OnionTex {
    pub cut: NodeId,
    pub tex: TextureHandle,
    pub uv_max: [f32; 2],
}

pub(crate) struct AnimState {
    pub onion: OnionOpts,
    svc: Option<OnionService>,
    /// Assinatura do último pedido feito à thread e a desejada agora (`None` = nada a mostrar).
    asked: Option<u64>,
    want: Option<u64>,
    pub onion_tex: Option<OnionTex>,
    /// Vizinhos renderizados pela thread (diagnóstico e testes).
    pub onion_renders: u64,
    density: HashMap<WorkspaceId, Density>,
    /// Pastas recolhidas na linha do tempo.
    pub collapsed: HashSet<NodeId>,
    /// Cels-chave destravados à mão no douga (só nesta sessão).
    pub unlocked: HashSet<NodeId>,
    pub tl: TlMetrics,
    pub char_ref: super::panels::char_ref::CharRefState,
    pub scene: super::panels::scene_info::SceneCache,
}

impl Default for AnimState {
    fn default() -> Self {
        AnimState {
            onion: OnionOpts::default(),
            svc: None,
            asked: None,
            want: None,
            onion_tex: None,
            onion_renders: 0,
            density: HashMap::new(),
            collapsed: HashSet::new(),
            unlocked: HashSet::new(),
            tl: TlMetrics { header_w: 150.0, ruler_h: 22.0, row_h: 22.0 },
            char_ref: Default::default(),
            scene: Default::default(),
        }
    }
}

impl AnimState {
    /// Solta a textura da mesa de luz (orçamento de VRAM, `gpu_owners`); o próximo quadro com ela ligada pede de novo.
    pub(crate) fn release_onion(&mut self) -> bool {
        self.asked = None;
        self.onion_tex.take().is_some()
    }

    /// Densidade da linha do tempo no workspace: a Animação começa compacta (a linha do tempo ocupava área demais).
    pub(crate) fn density(&self, ws: WorkspaceId) -> Density {
        self.density.get(&ws).copied().unwrap_or(if ws == WorkspaceId::Animacao { Density::Compact } else { Density::Normal })
    }

    pub(crate) fn toggle_density(&mut self, ws: WorkspaceId) {
        let d = match self.density(ws) {
            Density::Normal => Density::Compact,
            Density::Compact => Density::Normal,
        };
        self.density.insert(ws, d);
    }

    /// A mesa de luz está mostrando algo agora (para a interface e a bridge).
    pub(crate) fn onion_visible(&self) -> bool {
        self.onion.on && self.want.is_some() && self.onion_tex.is_some()
    }
}

// ------------------------------------------------------------------------------------------------ fase do corte

pub(crate) fn cut_phase(p: &Project, cut: &NodeId) -> CutPhase {
    cmds::cut_data(p, cut).map(|c| c.phase).unwrap_or_default()
}

pub(crate) fn phase_key(ph: CutPhase) -> &'static str {
    match ph {
        CutPhase::Genga => "anim.phase.genga",
        CutPhase::Douga => "anim.phase.douga",
    }
}

/// Variante do layout da Animação para o corte ativo.
pub(crate) fn layout_variant(core: &CoreSession, ed: &mut Editor) -> LayoutVariant {
    match ed.active_cut(core).map(|c| cut_phase(&core.project, &c)) {
        Some(CutPhase::Douga) => LayoutVariant::Douga,
        _ => LayoutVariant::Base,
    }
}

/// Muda a fase do corte (um passo de desfazer) e põe a mesa de luz no padrão da fase: genga vê a chave anterior e a
/// seguinte; douga, dois desenhos antes e dois depois.
pub(crate) fn set_phase(env: &mut Env, cut: &NodeId, phase: CutPhase) {
    if cut_phase(&env.core.project, cut) == phase {
        return;
    }
    if let Err(e) = cmds::set_cut_props(env.core, cut, json!({ "phase": phase })) {
        env.ed.notify(e);
        return;
    }
    let o = &mut env.ed.anim.onion;
    match phase {
        CutPhase::Genga => {
            o.mode = OnionMode::Keys;
            o.before = 1;
            o.after = 1;
        }
        CutPhase::Douga => {
            o.mode = OnionMode::Drawings;
            o.before = 2;
            o.after = 2;
        }
    }
}

fn has_content(c: &CelData) -> bool {
    !c.strokes.is_empty() || !c.tiles.is_empty() || !c.fills.is_empty() || c.image.is_some() || !c.text.is_empty()
}

/// O cel é uma chave com desenho que não foi destravada (a trava só vale no douga; quem chama confere a fase).
pub(crate) fn key_cel_locked(p: &Project, unlocked: &HashSet<NodeId>, cel: &NodeId) -> bool {
    cmds::cel_data(p, cel).is_some_and(|cd| cd.role == CelRole::Key && has_content(cd) && !unlocked.contains(cel))
}

/// Cel-chave travado: no douga, o cel exposto na camada ativa é chave, já tem desenho e não foi destravado. Cel-chave
/// vazio não trava (não há o que estragar; é o caso do cel recém-criado).
pub(crate) fn locked_key_cel(core: &CoreSession, ed: &mut Editor) -> Option<NodeId> {
    let cut = ed.active_cut(core)?;
    if cut_phase(&core.project, &cut) != CutPhase::Douga {
        return None;
    }
    let layer = ed.active_layer(core)?;
    let cel = cmds::exposed_cel(&core.project, &cut, &layer, ed.frame)?;
    key_cel_locked(&core.project, &ed.anim.unlocked, &cel).then_some(cel)
}

/// Chamado antes de um gesto que edita o desenho: no cel-chave travado, avisa e impede.
pub(crate) fn edit_blocked(env: &mut Env) -> bool {
    if locked_key_cel(env.core, env.ed).is_some() {
        env.ed.notify(tr("anim.lock.blocked"));
        return true;
    }
    false
}

// ------------------------------------------------------------------------------------------------ papel e navegação

pub(crate) fn next_role(r: CelRole) -> CelRole {
    match r {
        CelRole::Key => CelRole::Breakdown,
        CelRole::Breakdown => CelRole::Inbetween,
        CelRole::Inbetween => CelRole::Key,
    }
}

/// Cor da marca do papel (K vermelha, B amarela, I azul, como nas folhas de exposição).
pub(crate) fn role_color(r: CelRole) -> Color32 {
    match r {
        CelRole::Key => Color32::from_rgb(226, 72, 60),
        CelRole::Breakdown => Color32::from_rgb(232, 178, 40),
        CelRole::Inbetween => Color32::from_rgb(70, 140, 230),
    }
}

/// Marca do papel: ● chave, ◐ breakdown, ○ intercalação.
pub(crate) fn paint_role_mark(painter: &egui::Painter, c: Pos2, r: f32, role: CelRole) {
    let col = role_color(role);
    match role {
        CelRole::Key => {
            painter.circle_filled(c, r, col);
        }
        CelRole::Breakdown => {
            painter.circle_stroke(c, r - 0.6, egui::Stroke::new(1.2, col));
            let n = 10;
            let mut pts = vec![c];
            for k in 0..=n {
                let a = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * k as f32 / n as f32;
                pts.push(c + egui::vec2(a.cos(), a.sin()) * r);
            }
            painter.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
        }
        CelRole::Inbetween => {
            painter.circle_stroke(c, r - 0.6, egui::Stroke::new(1.4, col));
        }
    }
}

/// Cel atual da camada ativa: o exposto no quadro (ou o escolhido no painel, se nada está exposto).
pub(crate) fn current_cel(core: &CoreSession, ed: &mut Editor) -> Option<NodeId> {
    let cut = ed.active_cut(core)?;
    let layer = ed.active_layer(core)?;
    cmds::exposed_cel(&core.project, &cut, &layer, ed.frame).or_else(|| ed.cel_number.and_then(|n| core.project.cel_by_number(&layer, n)).map(|n| n.id.clone()))
}

/// `K`: chave → breakdown → intercalação → chave no cel atual (um passo de desfazer).
pub(crate) fn cycle_role(env: &mut Env) {
    let Some(cel) = current_cel(env.core, env.ed) else {
        env.ed.notify(tr("anim.no_cel"));
        return;
    };
    let Some(cd) = cmds::cel_data(&env.core.project, &cel).cloned() else { return };
    let role = next_role(cd.role);
    match cmds::set_cel_role(env.core, &cel, role) {
        Ok(()) => env.ed.notify(format!("{} {}: {}", tr("layers.cel"), cd.number, tr(cmds::cel_role_key(role)))),
        Err(e) => env.ed.notify(e),
    }
}

/// Começos de desenho da camada: (quadro, cel, papel) de cada bloco de exposição com cel (os vazios ficam de fora).
/// Camada sem trilha expõe o menor cel do começo ao fim.
pub(crate) fn drawing_starts(p: &Project, cut: &NodeId, layer: &NodeId) -> Vec<(u32, NodeId, CelRole)> {
    let Some(cd) = cmds::cut_data(p, cut) else { return Vec::new() };
    let role_of = |n: &Node| match &n.data {
        NodeKind::Cel(c) => Some(c.role),
        _ => None,
    };
    match cd.xsheet.track_for_layer(layer) {
        Some(track) => exposure_runs(track, cd.duration)
            .into_iter()
            .filter_map(|(a, _, c)| match c {
                CellRef::Cel { number } => {
                    let n = p.cel_by_number(layer, number)?;
                    Some((a, n.id.clone(), role_of(n)?))
                }
                CellRef::Empty => None,
            })
            .collect(),
        None => cmds::layer_cels(p, layer).first().and_then(|(_, id)| Some((0, id.clone(), role_of(p.node(id)?)?))).into_iter().collect(),
    }
}

/// Próximo (ou anterior) começo de desenho a partir de `frame`; com `keys_only`, só os desenhos-chave.
pub(crate) fn jump_target(starts: &[(u32, NodeId, CelRole)], frame: u32, forward: bool, keys_only: bool) -> Option<u32> {
    let frames = starts.iter().filter(|s| !keys_only || s.2 == CelRole::Key).map(|s| s.0);
    if forward { frames.filter(|f| *f > frame).min() } else { frames.filter(|f| *f < frame).max() }
}

/// Camadas de animação do corte (vetor e raster visíveis), de cima para baixo.
pub(crate) fn animation_layers(p: &Project, cut: &NodeId) -> Vec<NodeId> {
    cmds::layer_rows(p, cut).into_iter().map(|r| r.id).filter(|id| is_anim_layer(p, id)).collect()
}

fn is_anim_layer(p: &Project, id: &NodeId) -> bool {
    cmds::layer_data(p, id).is_some_and(|l| l.visible && matches!(l.kind, LayerType::Vector | LayerType::Raster))
}

/// `Ctrl+→/←` (chave) e `Alt+→/←` (desenho): vai ao começo do próximo/anterior desenho da camada ativa; se ela não tem
/// para onde ir, olha todas as camadas de animação do corte.
pub(crate) fn jump(env: &mut Env, forward: bool, keys_only: bool) {
    let Some(cut) = env.ed.active_cut(env.core) else { return };
    let p = &env.core.project;
    let frame = env.ed.frame;
    let active = env.ed.active_layer(env.core);
    let mut target = active.as_ref().and_then(|l| jump_target(&drawing_starts(p, &cut, l), frame, forward, keys_only));
    if target.is_none() {
        let all: Vec<(u32, NodeId, CelRole)> = animation_layers(p, &cut).iter().flat_map(|l| drawing_starts(p, &cut, l)).collect();
        target = jump_target(&all, frame, forward, keys_only);
    }
    match target {
        Some(f) => {
            env.ed.playback.playing = false;
            env.ed.frame = f;
        }
        None => env.ed.notify(tr(if keys_only { "anim.no_key" } else { "anim.no_drawing" })),
    }
}

// ------------------------------------------------------------------------------------------------ atalhos

/// Teclas da animação (configuráveis em Preferências > Atalhos). Rodam antes das do editor.
pub(crate) fn shortcuts(ctx: &egui::Context, env: &mut Env) {
    if ctx.egui_wants_keyboard_input() || egui::Popup::is_any_open(ctx) || super::text_tool::editing(env.ed) {
        return;
    }
    use crate::keymap::Action as A;
    let keys = env.ed.keys.clone();
    let hits: Vec<A> = ctx.input_mut(|i| {
        [A::CelRoleCycle, A::KeyNext, A::KeyPrev, A::DrawingNext, A::DrawingPrev, A::OnionToggle, A::TimelineDensity].into_iter().filter(|a| keys.get(*a).is_some_and(|c| c.take(i))).collect()
    });
    for a in hits {
        match a {
            A::CelRoleCycle => cycle_role(env),
            A::KeyNext => jump(env, true, true),
            A::KeyPrev => jump(env, false, true),
            A::DrawingNext => jump(env, true, false),
            A::DrawingPrev => jump(env, false, false),
            A::OnionToggle => {
                let o = &mut env.ed.anim.onion;
                o.on = !o.on;
                let msg = tr(if o.on { "anim.onion.on" } else { "anim.onion.off" });
                env.ed.notify(msg);
            }
            A::TimelineDensity => env.ed.anim.toggle_density(env.ws),
            _ => {}
        }
    }
}

// ------------------------------------------------------------------------------------------------ mesa de luz: plano

/// Um vizinho a mostrar: o cel `cel` da camada, renderizado no quadro `frame` (onde ele está exposto), do lado
/// anterior ou seguinte, na distância `dist` (1 = o mais perto).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OnionItem {
    pub layer: NodeId,
    pub cel: NodeId,
    pub frame: u32,
    pub next: bool,
    pub dist: u32,
}

/// Camadas da mesa de luz: a ativa (se for de animação) ou todas as de animação.
pub(crate) fn onion_layers(p: &Project, cut: &NodeId, active: Option<&NodeId>, which: OnionLayers) -> Vec<NodeId> {
    match which {
        OnionLayers::Active => active.filter(|a| is_anim_layer(p, a)).cloned().into_iter().collect(),
        OnionLayers::Animation => animation_layers(p, cut),
    }
}

/// Os vizinhos de cada camada no quadro `frame` segundo as opções (sem repetir o cel atual nem o mesmo cel duas vezes).
pub(crate) fn onion_plan(p: &Project, cut: &NodeId, layers: &[NodeId], frame: u32, o: &OnionOpts) -> Vec<OnionItem> {
    let Some(cd) = cmds::cut_data(p, cut) else { return Vec::new() };
    let dur = cd.duration;
    let mut out = Vec::new();
    for layer in layers {
        let current = cmds::exposed_cel(p, cut, layer, frame);
        let mut seen: HashSet<NodeId> = current.iter().cloned().collect();
        match o.mode {
            OnionMode::Frames => {
                for (next, n) in [(false, o.before), (true, o.after)] {
                    for k in 1..=n.min(ONION_MAX) {
                        let f = if next { frame + k } else if let Some(f) = frame.checked_sub(k) { f } else { break };
                        if f >= dur {
                            break;
                        }
                        if let Some(c) = cmds::exposed_cel(p, cut, layer, f) {
                            if seen.insert(c.clone()) {
                                out.push(OnionItem { layer: layer.clone(), cel: c, frame: f, next, dist: k });
                            }
                        }
                    }
                }
            }
            OnionMode::Drawings | OnionMode::Keys => {
                let starts = drawing_starts(p, cut, layer);
                let keys_only = o.mode == OnionMode::Keys;
                // o bloco do quadro atual: o último que começou até aqui (se ele é o cel exposto; num quadro vazio é anterior)
                let (prev, next): (Vec<&(u32, NodeId, CelRole)>, Vec<&(u32, NodeId, CelRole)>) = match starts.iter().rposition(|s| s.0 <= frame) {
                    Some(i) if current.as_ref() == Some(&starts[i].1) => (starts[..i].iter().rev().collect(), starts[i + 1..].iter().collect()),
                    Some(i) => (starts[..=i].iter().rev().collect(), starts[i + 1..].iter().collect()),
                    None => (Vec::new(), starts.iter().collect()),
                };
                for (is_next, list, n) in [(false, prev, o.before), (true, next, o.after)] {
                    let mut k = 0;
                    for (f, cel, role) in list {
                        if k >= n.min(ONION_MAX) {
                            break;
                        }
                        if keys_only && *role != CelRole::Key {
                            continue;
                        }
                        if seen.insert(cel.clone()) {
                            k += 1;
                            out.push(OnionItem { layer: layer.clone(), cel: cel.clone(), frame: *f, next: is_next, dist: k });
                        }
                    }
                }
            }
        }
    }
    out
}

/// Opacidade de um vizinho: a escolhida no mais perto, caindo em linha reta até 1/n dela no mais longe.
pub(crate) fn onion_opacity(o: &OnionOpts, it: &OnionItem) -> f32 {
    let n = if it.next { o.after } else { o.before }.max(1) as f32;
    (o.opacity * (1.0 - (it.dist as f32 - 1.0) / n)).clamp(0.0, 1.0)
}

// ------------------------------------------------------------------------------------------------ mesa de luz: thread

/// O que determina a silhueta de um cel: camada, cel e revisão dele, posição da camada (deslocamento e chaves de
/// movimento; o quadro só conta quando a camada se move) e a redução.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct SilKey {
    layer: NodeId,
    cel: NodeId,
    rev: u64,
    place: u64,
    frame_key: u32,
    reduce: u32,
}

#[derive(Clone, Debug)]
struct SilReq {
    key: SilKey,
    frame: u32,
}

struct Job {
    sig: u64,
    snap: Arc<Snapshot>,
    cut: NodeId,
    canvas: (u32, u32),
    reduce: u32,
    /// Vizinhos do mais longe ao mais perto (os de perto ficam por cima), com tinta e opacidade.
    items: Vec<(SilReq, [u8; 3], f32)>,
    /// Desenho atual de cada camada: o que ele cobre sai da textura (a mesa de luz fica por baixo dele).
    masks: Vec<SilReq>,
}

struct Done {
    sig: u64,
    cut: NodeId,
    image: Result<(ColorImage, [f32; 2]), String>,
    renders: u64,
}

#[derive(Default)]
struct Queue {
    job: Option<Job>,
    shutdown: bool,
}

struct OnionService {
    shared: Arc<(Mutex<Queue>, Condvar)>,
    rx: crossbeam_channel::Receiver<Done>,
}

impl OnionService {
    fn new(ctx: &egui::Context) -> Self {
        let shared = Arc::new((Mutex::new(Queue::default()), Condvar::new()));
        let (tx, rx) = crossbeam_channel::unbounded();
        let (s2, ctx2) = (Arc::clone(&shared), ctx.clone());
        let _ = std::thread::Builder::new().name("aiai-onion".into()).spawn(move || onion_worker(s2, tx, ctx2));
        OnionService { shared, rx }
    }

    /// O pedido novo substitui o que ainda não começou.
    fn submit(&self, job: Job) {
        if let Ok(mut q) = self.shared.0.lock() {
            q.job = Some(job);
        }
        self.shared.1.notify_one();
    }
}

impl Drop for OnionService {
    fn drop(&mut self) {
        if let Ok(mut q) = self.shared.0.lock() {
            q.shutdown = true;
            q.job = None;
        }
        self.shared.1.notify_all();
    }
}

fn onion_worker(shared: Arc<(Mutex<Queue>, Condvar)>, tx: crossbeam_channel::Sender<Done>, ctx: egui::Context) {
    let mut cache: HashMap<SilKey, Arc<Vec<u8>>> = HashMap::new();
    loop {
        let job = {
            let Ok(mut q) = shared.0.lock() else { return };
            loop {
                if q.shutdown {
                    return;
                }
                if let Some(j) = q.job.take() {
                    break j;
                }
                q = match shared.1.wait(q) {
                    Ok(q) => q,
                    Err(_) => return,
                };
            }
        };
        let mut renders = 0;
        let image = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_job(&job, &mut cache, &mut renders))).unwrap_or_else(|_| {
            cache.clear();
            Err("a mesa de luz entrou em pânico".to_owned())
        });
        if tx.send(Done { sig: job.sig, cut: job.cut.clone(), image, renders }).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

/// Alfa (0..255) do cel sozinho, na redução pedida.
fn silhouette(job: &Job, req: &SilReq) -> Result<Vec<u8>, String> {
    let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: Some(vec![req.key.layer.clone()]), background: None, camera: false, layer_motion: true };
    let out = aiai_render::render_frame(&job.snap.project, &job.snap.blobs, &job.cut, req.frame, &opts).map_err(|e| e.to_string())?;
    let (w, h) = (out.image.width as usize, out.image.height as usize);
    let r = job.reduce.max(1) as usize;
    let a8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    if r == 1 {
        return Ok(out.image.data.iter().map(|p| a8(p[3])).collect());
    }
    let (nw, nh) = (w.div_ceil(r), h.div_ceil(r));
    let mut v = Vec::with_capacity(nw * nh);
    for ty in 0..nh {
        for tx in 0..nw {
            let (x0, y0) = (tx * r, ty * r);
            let (x1, y1) = ((x0 + r).min(w), (y0 + r).min(h));
            let mut acc = 0.0;
            for y in y0..y1 {
                for x in x0..x1 {
                    acc += out.image.data[y * w + x][3];
                }
            }
            v.push(a8(acc / ((x1 - x0) * (y1 - y0)) as f32));
        }
    }
    Ok(v)
}

fn run_job(job: &Job, cache: &mut HashMap<SilKey, Arc<Vec<u8>>>, renders: &mut u64) -> Result<(ColorImage, [f32; 2]), String> {
    let r = job.reduce.max(1) as usize;
    let (w, h) = (job.canvas.0 as usize, job.canvas.1 as usize);
    let (nw, nh) = (w.div_ceil(r), h.div_ceil(r));
    let n = nw * nh;
    let mut get = |req: &SilReq| -> Result<Arc<Vec<u8>>, String> {
        if let Some(s) = cache.get(&req.key) {
            return Ok(Arc::clone(s));
        }
        let s = Arc::new(silhouette(job, req)?);
        *renders += 1;
        cache.insert(req.key.clone(), Arc::clone(&s));
        Ok(s)
    };
    let mut out = vec![[0u32; 4]; n];
    for (req, tint, op) in &job.items {
        let sil = get(req)?;
        if sil.len() != n {
            return Err(format!("silhueta com tamanho inesperado ({} ≠ {n})", sil.len()));
        }
        let k = (op.clamp(0.0, 1.0) * 255.0) as u32;
        for (o, &a) in out.iter_mut().zip(sil.iter()) {
            let a = a as u32 * k / 255;
            if a == 0 {
                continue;
            }
            let inv = 255 - a;
            for c in 0..3 {
                o[c] = (tint[c] as u32 * a + o[c] * inv) / 255;
            }
            o[3] = a + o[3] * inv / 255;
        }
    }
    for req in &job.masks {
        let sil = get(req)?;
        if sil.len() != n {
            continue;
        }
        for (o, &m) in out.iter_mut().zip(sil.iter()) {
            if m == 0 {
                continue;
            }
            let keep = 255 - m as u32;
            for v in o.iter_mut() {
                *v = *v * keep / 255;
            }
        }
    }
    // cache: fica o que este pedido usou e, se couber, o resto (vizinhos de quadros perto voltam logo)
    if cache.len() > 48 {
        let used: HashSet<&SilKey> = job.items.iter().map(|(r, _, _)| &r.key).chain(job.masks.iter().map(|r| &r.key)).collect();
        cache.retain(|k, _| used.contains(k));
    }
    let pixels = out.iter().map(|p| Color32::from_rgba_premultiplied(p[0] as u8, p[1] as u8, p[2] as u8, p[3] as u8)).collect();
    let uv_max = [w as f32 / r as f32 / nw as f32, h as f32 / r as f32 / nh as f32];
    Ok((ColorImage { size: [nw, nh], source_size: egui::vec2(nw as f32, nh as f32), pixels }, uv_max))
}

// ------------------------------------------------------------------------------------------------ mesa de luz: canvas

/// Posição da camada para a silhueta: assinatura do deslocamento e das chaves de movimento, e o quadro só quando a
/// camada tem chaves (sem chaves o cel sai igual em qualquer quadro).
fn placement(p: &Project, layer: &NodeId, frame: u32) -> (u64, u32) {
    let Some(l) = cmds::layer_data(p, layer) else { return (0, frame) };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    format!("{:?}|{:?}|{}", l.offset, l.keys, l.opacity).hash(&mut h);
    (h.finish(), if l.keys.is_empty() { u32::MAX } else { frame })
}

fn sil_req(p: &Project, layer: &NodeId, cel: &NodeId, frame: u32, reduce: u32) -> SilReq {
    let rev = p.node(cel).map(|n| n.provenance.revision).unwrap_or(0);
    let (place, frame_key) = placement(p, layer, frame);
    SilReq { key: SilKey { layer: layer.clone(), cel: cel.clone(), rev, place, frame_key, reduce }, frame }
}

/// Uma vez por quadro do canvas: recolhe a textura pronta e, se os vizinhos/opções mudaram, pede a nova à thread.
pub(crate) fn onion_frame(env: &mut Env, ctx: &egui::Context, cut: &NodeId, canvas: (u32, u32)) {
    {
        let st = &mut env.ed.anim;
        if let Some(svc) = &st.svc {
            while let Ok(d) = svc.rx.try_recv() {
                st.onion_renders += d.renders;
                match d.image {
                    Ok((img, uv_max)) => match &mut st.onion_tex {
                        Some(t) => {
                            t.tex.set(img, TextureOptions::LINEAR);
                            t.cut = d.cut;
                            t.uv_max = uv_max;
                        }
                        None => st.onion_tex = Some(OnionTex { tex: ctx.load_texture("onion-skin", img, TextureOptions::LINEAR), cut: d.cut, uv_max }),
                    },
                    Err(e) => tracing::warn!("mesa de luz (pedido {:x}): {e}", d.sig),
                }
            }
        }
    }
    if !env.ed.anim.onion.on || env.ed.playback.playing {
        return;
    }
    let active = env.ed.active_layer(env.core);
    let p = &env.core.project;
    let o = env.ed.anim.onion;
    let frame = env.ed.frame;
    let layers = onion_layers(p, cut, active.as_ref(), o.layers);
    let items = onion_plan(p, cut, &layers, frame, &o);
    if items.is_empty() {
        env.ed.anim.want = None;
        return;
    }
    let max_side = ctx.input(|i| i.max_texture_side) as u32;
    let big = canvas.0 as u64 * canvas.1 as u64 > 2_400_000;
    let reduce = pick_reduce(canvas.0, canvas.1, if big { 2 } else { 1 }, max_side);
    // do mais longe ao mais perto: anteriores, depois seguintes
    let mut sorted: Vec<&OnionItem> = items.iter().collect();
    sorted.sort_by_key(|it| (it.next, std::cmp::Reverse(it.dist)));
    let rgb = |c: Color32| [c.r(), c.g(), c.b()];
    let job_items: Vec<(SilReq, [u8; 3], f32)> =
        sorted.iter().map(|it| (sil_req(p, &it.layer, &it.cel, it.frame, reduce), rgb(if it.next { o.next } else { o.prev }), onion_opacity(&o, it))).collect();
    let masks: Vec<SilReq> = layers.iter().filter_map(|l| cmds::exposed_cel(p, cut, l, frame).map(|c| sil_req(p, l, &c, frame, reduce))).collect();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (cut, canvas, reduce).hash(&mut h);
    for (r, tint, op) in &job_items {
        (&r.key, tint, op.to_bits()).hash(&mut h);
    }
    for r in &masks {
        r.key.hash(&mut h);
    }
    let sig = h.finish();
    env.ed.anim.want = Some(sig);
    if env.ed.anim.asked == Some(sig) {
        return;
    }
    let snap = match env.ed.render.as_mut() {
        Some(r) => r.snapshot(env.core),
        None => Arc::new(Snapshot { project: env.core.project.clone(), blobs: env.core.blobs.clone() }),
    };
    let st = &mut env.ed.anim;
    st.svc.get_or_insert_with(|| OnionService::new(ctx)).submit(Job { sig, snap, cut: cut.clone(), canvas, reduce, items: job_items, masks });
    st.asked = Some(sig);
}

/// Desenha a mesa de luz no canvas (cantos do quadro na tela em `sp`), se ligada e pronta para este corte.
pub(crate) fn paint_onion(ed: &Editor, painter: &egui::Painter, sp: [Pos2; 4], cut: &NodeId) {
    let st = &ed.anim;
    if !st.onion.on || ed.playback.playing || st.want.is_none() {
        return;
    }
    let Some(t) = st.onion_tex.as_ref().filter(|t| t.cut == *cut) else { return };
    let [u, v] = t.uv_max;
    let uv = [Pos2::new(0.0, 0.0), Pos2::new(u, 0.0), Pos2::new(u, v), Pos2::new(0.0, v)];
    let mut m = egui::Mesh::with_texture(t.tex.id());
    for (p, uv) in sp.iter().zip(uv.iter()) {
        m.vertices.push(egui::epaint::Vertex { pos: *p, uv: *uv, color: Color32::WHITE });
    }
    m.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    painter.add(egui::Shape::mesh(m));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::commands::{Exposure, add_cel, add_layer, quick_start, set_exposure};
    use aiai_core::command::IndexedStroke;

    fn line(id: &str, pts: &[(f32, f32)], width: f32) -> Stroke {
        Stroke {
            id: Id::from(id),
            points: pts.iter().map(|(x, y)| StrokePoint { x: *x, y: *y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }).collect(),
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width,
            profile: WidthProfile::default(),
            color: ColorRef::Direct { rgba: Rgba::BLACK },
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
        }
    }

    /// Corte de 24 quadros com uma camada vetorial: cels 1..4 expostos em 0, 6, 12, 18 (cada um com um traço).
    fn doc() -> (CoreSession, NodeId, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let cut = quick_start(&mut core, 24, None).unwrap();
        let a = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        for _ in 0..3 {
            add_cel(&mut core, &a, None).unwrap();
        }
        for (i, f) in [0u32, 6, 12, 18].iter().enumerate() {
            set_exposure(&mut core, &cut, &a, *f, Exposure::Cel(i as u32 + 1)).unwrap();
        }
        for (n, id) in cmds::layer_cels(&core.project, &a) {
            let x = 8.0 + n as f32 * 10.0;
            let s = line(&format!("s{n}"), &[(x, 5.0), (x, 30.0)], 3.0);
            cmds::run(&mut core, Command::InsertStrokes { cel: id, items: vec![IndexedStroke { index: 0, stroke: s }] }).unwrap();
        }
        (core, cut, a)
    }

    fn cel(core: &CoreSession, layer: &NodeId, n: u32) -> NodeId {
        core.project.cel_by_number(layer, n).unwrap().id.clone()
    }

    #[test]
    fn roles_cycle_and_keys_are_found() {
        assert_eq!(next_role(CelRole::Key), CelRole::Breakdown);
        assert_eq!(next_role(next_role(next_role(CelRole::Key))), CelRole::Key);
        let (mut core, cut, a) = doc();
        // 1 e 4 chaves, 2 breakdown, 3 intercalação
        { let c = cel(&core, &a, 2); cmds::set_cel_role(&mut core, &c, CelRole::Breakdown).unwrap(); }
        { let c = cel(&core, &a, 3); cmds::set_cel_role(&mut core, &c, CelRole::Inbetween).unwrap(); }
        let starts = drawing_starts(&core.project, &cut, &a);
        assert_eq!(starts.iter().map(|s| s.0).collect::<Vec<_>>(), vec![0, 6, 12, 18]);
        assert_eq!(jump_target(&starts, 0, true, true), Some(18), "Ctrl+→ pula a breakdown e a intercalação");
        assert_eq!(jump_target(&starts, 0, true, false), Some(6), "Alt+→ vai ao próximo desenho");
        assert_eq!(jump_target(&starts, 14, false, true), Some(0));
        assert_eq!(jump_target(&starts, 14, false, false), Some(12));
        assert_eq!(jump_target(&starts, 18, true, true), None);
    }

    #[test]
    fn onion_plan_by_frames_drawings_and_keys() {
        let (mut core, cut, a) = doc();
        let p = &core.project;
        let layers = vec![a.clone()];
        let mut o = OnionOpts { on: true, before: 2, after: 2, mode: OnionMode::Drawings, ..Default::default() };
        // quadro 13 (cel 3): desenhos 2 e 1 antes, 4 depois
        let plan = onion_plan(p, &cut, &layers, 13, &o);
        let got: Vec<(u32, bool, u32, u32)> = plan.iter().map(|it| (p.node(&it.cel).and_then(|n| match &n.data { NodeKind::Cel(c) => Some(c.number), _ => None }).unwrap(), it.next, it.dist, it.frame)).collect();
        assert_eq!(got, vec![(2, false, 1, 6), (1, false, 2, 0), (4, true, 1, 18)]);
        // por quadros: nos quadros vizinhos (12, 11, 14, 15) só aparece o cel 2 (quadro 11); o cel 3 é o atual
        o.mode = OnionMode::Frames;
        let plan = onion_plan(p, &cut, &layers, 12, &o);
        assert_eq!(plan.len(), 1);
        assert_eq!((plan[0].frame, plan[0].next, plan[0].dist), (11, false, 1));
        // só chaves: com 2 e 3 marcados como não-chave, do quadro 13 vê o 1 antes e o 4 depois
        { let c = cel(&core, &a, 2); cmds::set_cel_role(&mut core, &c, CelRole::Breakdown).unwrap(); }
        { let c = cel(&core, &a, 3); cmds::set_cel_role(&mut core, &c, CelRole::Inbetween).unwrap(); }
        o.mode = OnionMode::Keys;
        let p = &core.project;
        let plan = onion_plan(p, &cut, &layers, 13, &o);
        assert_eq!(plan.iter().map(|it| (it.frame, it.next, it.dist)).collect::<Vec<_>>(), vec![(0, false, 1), (18, true, 1)]);
        // opacidade cai com a distância
        let near = OnionItem { layer: a.clone(), cel: cel(&core, &a, 1), frame: 0, next: false, dist: 1 };
        let far = OnionItem { dist: 2, ..near.clone() };
        assert!(onion_opacity(&o, &near) > onion_opacity(&o, &far));
        // camada escondida não entra na mesa de luz
        cmds::set_layer_props(&mut core, &a, json!({ "visible": false })).unwrap();
        assert!(onion_layers(&core.project, &cut, Some(&a), OnionLayers::Active).is_empty());
    }

    #[test]
    fn the_light_table_renders_neighbours_once_and_keeps_out_of_renders() {
        let (mut core, cut, a) = doc();
        let ctx = egui::Context::default();
        let mut ed = Editor::default();
        ed.cut = Some(cut.clone());
        ed.layer = Some(a.clone());
        ed.frame = 13;
        ed.anim.onion = OnionOpts { on: true, before: 1, after: 1, mode: OnionMode::Drawings, opacity: 1.0, ..Default::default() };
        let mut icons = crate::icons::Icons::default();
        let mut notes = Vec::new();
        let mut tool = crate::workspace::ToolId::Pen;
        let render_before = aiai_render::render_frame(&core.project, &core.blobs, &cut, 13, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image;
        let wait = |ed: &mut Editor, core: &mut CoreSession, icons: &mut crate::icons::Icons, notes: &mut Vec<(String, egui::Rect)>, tool: &mut crate::workspace::ToolId| {
            let t0 = std::time::Instant::now();
            loop {
                let mut env = Env { t: crate::theme::Tokens::dark(), core, ed, icons, notes, tool, ws: WorkspaceId::Animacao };
                onion_frame(&mut env, &ctx, &cut, (64, 36));
                if env.ed.anim.onion_tex.is_some() && env.ed.anim.asked == env.ed.anim.want && env.ed.anim.svc.as_ref().is_some_and(|s| s.rx.is_empty()) && env.ed.anim.onion_renders > 0 {
                    // espera a resposta do último pedido
                    std::thread::sleep(std::time::Duration::from_millis(30));
                    let mut env = Env { t: crate::theme::Tokens::dark(), core: env.core, ed: env.ed, icons: env.icons, notes: env.notes, tool: env.tool, ws: WorkspaceId::Animacao };
                    onion_frame(&mut env, &ctx, &cut, (64, 36));
                    break;
                }
                assert!(t0.elapsed().as_secs() < 20, "a mesa de luz não respondeu");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        wait(&mut ed, &mut core, &mut icons, &mut notes, &mut tool);
        // dois vizinhos (cel 2 antes, cel 4 depois) + a máscara do atual = 3 silhuetas
        assert_eq!(ed.anim.onion_renders, 3);
        assert!(ed.anim.onion_visible());
        // desenhar no cel atual (3) não refaz os vizinhos: só a máscara nova
        let c3 = cel(&core, &a, 3);
        let s = line("novo", &[(2.0, 2.0), (60.0, 30.0)], 2.0);
        cmds::run(&mut core, Command::InsertStrokes { cel: c3, items: vec![IndexedStroke { index: 1, stroke: s }] }).unwrap();
        let before = ed.anim.onion_renders;
        wait(&mut ed, &mut core, &mut icons, &mut notes, &mut tool);
        assert_eq!(ed.anim.onion_renders - before, 1, "só a silhueta do cel editado");
        // a mesa de luz nunca entra no render do documento
        let render_after = aiai_render::render_frame(&core.project, &core.blobs, &cut, 13, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image;
        assert_eq!(render_after.width, render_before.width);
        let tinted = render_after.data.iter().any(|p| p[3] > 0.0 && (p[0] - p[1]).abs() > 0.2);
        assert!(!tinted, "o render não tem tinta vermelha/verde da mesa de luz");
    }

    #[test]
    fn douga_locks_drawn_key_cels_until_unlocked() {
        let (mut core, cut, a) = doc();
        let mut ed = Editor::default();
        ed.cut = Some(cut.clone());
        ed.layer = Some(a.clone());
        ed.frame = 0;
        assert_eq!(locked_key_cel(&core, &mut ed), None, "genga não trava");
        cmds::set_cut_props(&mut core, &cut, json!({ "phase": CutPhase::Douga })).unwrap();
        assert_eq!(cut_phase(&core.project, &cut), CutPhase::Douga);
        let c1 = cel(&core, &a, 1);
        assert_eq!(locked_key_cel(&core, &mut ed), Some(c1.clone()), "chave com desenho trava no douga");
        assert_eq!(layout_variant(&core, &mut ed), LayoutVariant::Douga);
        // intercalação não trava
        cmds::set_cel_role(&mut core, &c1, CelRole::Inbetween).unwrap();
        assert_eq!(locked_key_cel(&core, &mut ed), None);
        cmds::set_cel_role(&mut core, &c1, CelRole::Key).unwrap();
        // destravar à mão
        ed.anim.unlocked.insert(c1.clone());
        assert_eq!(locked_key_cel(&core, &mut ed), None);
        // chave vazia (cel novo) não trava
        let n = add_cel(&mut core, &a, None).unwrap();
        set_exposure(&mut core, &cut, &a, 20, Exposure::Cel(n)).unwrap();
        ed.frame = 21;
        assert_eq!(locked_key_cel(&core, &mut ed), None);
        // a fase volta a genga pelo desfazer e o JSON antigo (sem fase) abre como genga
        let mut cd = cmds::cut_data(&core.project, &cut).unwrap().clone();
        cd.phase = CutPhase::Genga;
        let v = serde_json::to_value(&cd).unwrap();
        assert!(v.get("phase").is_none(), "genga não é gravado (projetos antigos ficam iguais)");
        let back: CutData = serde_json::from_value(v).unwrap();
        assert_eq!(back.phase, CutPhase::Genga);
    }

    #[test]
    fn compact_is_the_default_only_in_animation() {
        let mut st = AnimState::default();
        assert_eq!(st.density(WorkspaceId::Animacao), Density::Compact);
        assert_eq!(st.density(WorkspaceId::Pintura), Density::Normal);
        st.toggle_density(WorkspaceId::Animacao);
        assert_eq!(st.density(WorkspaceId::Animacao), Density::Normal);
        assert_eq!(st.density(WorkspaceId::Composicao), Density::Normal);
    }
}
