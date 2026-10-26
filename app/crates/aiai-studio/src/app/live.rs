//! Prévia ao vivo do pincel e da borracha: enquanto o traço é arrastado, uma thread roda o MESMO caminho do commit
//! (`paint_on_cel` do pincel/borracha raster, `erase_vector_command` da borracha vetorial) num recorte da camada, e o
//! canvas mostra o resultado por cima do quadro. Antes a prévia era uma polilinha (pincel) ou uma faixa branca
//! (borracha) e o traço "mudava" ao soltar.
//!
//! - Caneta: o traço vetorial rasterizado pelo mesmo motor do render (largura, pontas, antisserrilhado), com as
//!   camadas de cima na frente, em vez do polígono desenhado pela interface.
//! - Pincel: o recorte pintado, só nos pixels que mudaram, com a opacidade da camada, com as camadas de cima na frente
//!   (renderizadas uma vez no começo do traço) e os efeitos de cor de cima (Grade) aplicados.
//! - Borracha: os pixels que mudaram recompostos como o render faz: o quadro das camadas visíveis de baixo (sobre o
//!   xadrez do canvas), a camada no estado depois do apagar (opacidade e modo dela) e as camadas de cima na frente.
//!   O mesmo resultado do commit (raster, traços vetoriais por trecho/interseção/inteiro e regiões de preenchimento).
//!   A base (baixo e cima) é preparada antes do clique enquanto a borracha está escolhida (`prefetch_erase`).
//!
//! Depois de soltar, a prévia fica até o render com o traço gravado chegar (sem "piscar").

use super::commands::NodeId;
use aiai_core::*;
use aiai_paint::Affine;
use aiai_tools::erase::EraseMode;
use aiai_paint::brush::BrushPoint;
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Mode {
    Pen,
    Brush,
    EraseRaster,
    EraseVector,
    /// Borracha em camada de preenchimento: as regiões que o caminho toca saem.
    EraseFill,
    /// Editar pontos: o traço editado redesenhado pelo motor, com o original escondido (o quadro sem ele embaixo).
    EditStroke,
}

/// O que a thread precisa para o gesto inteiro (tirado do documento uma vez, no começo do traço).
pub(crate) struct StartSpec {
    pub mode: Mode,
    /// Documento: render do que fica em cima (pincel) ou do quadro sem a camada (borracha) e corte dos traços vetoriais.
    pub project: Option<Project>,
    pub blobs: BlobStore,
    pub cut: NodeId,
    pub layer: NodeId,
    pub frame: u32,
    pub cel: Option<CelData>,
    pub canvas: (u32, u32),
    pub preset: aiai_paint::brush::BrushPreset,
    pub color: [f32; 4],
    pub seed: u64,
    pub layer_opacity: f32,
    /// cel → canvas (camada com movimento); `None` = identidade.
    pub fwd: Option<Affine>,
    pub guides: bool,
    pub reference: bool,
    /// Xadrez do canvas (cores e lado da casa em px do desenho) para o que fica transparente embaixo.
    pub checker: ([Color32; 2], f32),
    pub erase_radius: f32,
    pub erase_mode: EraseMode,
    /// Editar pontos: (cel, traço) que sai do quadro de baixo (o arrastado entra no lugar).
    pub hide: Option<(NodeId, Id)>,
    /// Seleção que limita o pincel/borracha raster: cobertura em px do canvas (o commit usa a mesma).
    pub clip: Option<Arc<(Vec<f32>, u32, u32)>>,
}

struct Req {
    pts: Vec<Vec<f32>>,
    path: Vec<[f32; 2]>,
    stroke: Option<Stroke>,
}

#[derive(Default)]
struct State {
    req: Option<Req>,
    done: bool,
}

struct Out {
    origin: [i32; 2],
    image: ColorImage,
    /// Pincel/borracha raster: pedaço da prévia do tamanho do canvas (atualiza só este retângulo da textura).
    partial: Option<[usize; 2]>,
    /// Tempo da thread para este resultado (ms).
    ms: f32,
    /// Já no espaço do canvas (camada com movimento redesenhada no lugar): desenhada sem a transformação do cel.
    canvas_space: bool,
}

pub(crate) struct Overlay {
    /// Canto do recorte no espaço do cel (px) e tamanho.
    pub origin: [i32; 2],
    pub size: [usize; 2],
    pub tex: TextureHandle,
    pub fwd: Option<Affine>,
}

#[derive(Default)]
pub(crate) struct Live {
    shared: Option<Arc<(Mutex<State>, Condvar)>>,
    rx: Option<crossbeam_channel::Receiver<Out>>,
    sent: usize,
    fwd: Option<Affine>,
    pub overlay: Option<Overlay>,
    started_for: Option<Instant>,
    mode: Option<Mode>,
    /// Medida: ms do último resultado e quantos chegaram neste traço.
    pub last_ms: f32,
    pub updates: u32,
    /// Depois de soltar: some quando o canvas mostrar um render com este contador (ou em 5 s).
    hold: Option<(u64, Instant)>,
    /// Base da borracha preparada (ou a caminho) para esta chave.
    prep: Option<(BaseKey, Slot)>,
}

/// Chave da base da borracha: o documento (revisão), o lugar e o que entra no render.
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct BaseKey {
    pub revision: u64,
    pub cut: NodeId,
    pub layer: NodeId,
    pub frame: u32,
    pub guides: bool,
    pub reference: bool,
}

impl BaseKey {
    fn of(p: &Project, spec: &StartSpec) -> BaseKey {
        BaseKey { revision: p.revision, cut: spec.cut.clone(), layer: spec.layer.clone(), frame: spec.frame, guides: spec.guides, reference: spec.reference }
    }
}

/// O que a borracha recompõe em volta da camada: o quadro das camadas visíveis de baixo (pré-multiplicado, sem
/// xadrez), o que fica na frente e como a camada entra (opacidade no quadro, modo).
pub(crate) struct EraseBase {
    below: Option<aiai_paint::Image>,
    above: Above,
    opacity: f32,
    blend: BlendMode,
}

/// Base sendo preparada numa thread: `None` = ainda não; `Some(None)` = falhou (quem espera faz a própria).
type Slot = Arc<(Mutex<Option<Option<Arc<EraseBase>>>>, Condvar)>;

fn wait_slot(s: &Slot) -> Option<Arc<EraseBase>> {
    let mut g = s.0.lock().ok()?;
    loop {
        if let Some(v) = g.as_ref() {
            return v.clone();
        }
        g = s.1.wait(g).ok()?;
    }
}

fn erase_mode(m: Mode) -> bool {
    matches!(m, Mode::EraseRaster | Mode::EraseVector | Mode::EraseFill)
}

impl Live {
    /// Gesto (pelo instante em que começou) para o qual a prévia já foi ligada.
    pub(crate) fn started_for(&self) -> Option<Instant> {
        self.started_for
    }

    pub(crate) fn start(&mut self, ctx: &egui::Context, spec: StartSpec, gesture: Instant) {
        self.stop();
        self.started_for = Some(gesture);
        self.mode = Some(spec.mode);
        self.updates = 0;
        self.overlay = None;
        self.hold = None;
        self.sent = 0;
        self.fwd = spec.fwd;
        // borracha: a base preparada antes do clique (pronta ou a caminho) serve se o documento e o lugar são os mesmos
        let slot = match &spec.project {
            Some(p) if erase_mode(spec.mode) => {
                let k = BaseKey::of(p, &spec);
                self.prep.as_ref().filter(|(pk, _)| *pk == k).map(|(_, s)| Arc::clone(s))
            }
            _ => None,
        };
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let (tx, rx) = crossbeam_channel::bounded(2);
        let (sh, ctx2) = (Arc::clone(&shared), ctx.clone());
        let spawned = std::thread::Builder::new().name("aiai-live".into()).spawn(move || worker(spec, slot, sh, tx, ctx2));
        if spawned.is_ok() {
            self.shared = Some(shared);
            self.rx = Some(rx);
        }
    }

    /// Manda o traço até aqui (o mais novo substitui o que não começou).
    pub(crate) fn mode(&self) -> Option<Mode> {
        self.mode.filter(|_| self.shared.is_some())
    }

    pub(crate) fn feed(&mut self, pts: Vec<Vec<f32>>, path: Vec<[f32; 2]>, stroke: Option<Stroke>, count: usize) {
        if count == self.sent {
            return;
        }
        let Some(sh) = &self.shared else { return };
        self.sent = count;
        if let Ok(mut st) = sh.0.lock() {
            st.req = Some(Req { pts, path, stroke });
        }
        sh.1.notify_one();
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context, shown_counter: Option<u64>) {
        if let Some(rx) = &self.rx {
            let mut got: Vec<Out> = Vec::new();
            while let Ok(o) = rx.try_recv() {
                // pedaços de uma prévia em tiles valem todos (cada um é um retângulo diferente); inteiras: só a última
                if o.partial.is_none() {
                    got.retain(|g| g.partial.is_some());
                }
                got.push(o);
            }
            for o in got {
                self.last_ms = o.ms;
                self.updates += 1;
                match o.partial {
                    Some(full) => {
                        let ov = match self.overlay.take() {
                            Some(ov) if ov.size == full && ov.origin == [0, 0] => ov,
                            _ => {
                                let blank = ColorImage::new(full, vec![Color32::TRANSPARENT; full[0] * full[1]]);
                                Overlay { origin: [0, 0], size: full, tex: ctx.load_texture("live-stroke", blank, TextureOptions::LINEAR), fwd: self.fwd }
                            }
                        };
                        let mut ov = ov;
                        ov.tex.set_partial([o.origin[0].max(0) as usize, o.origin[1].max(0) as usize], o.image, TextureOptions::LINEAR);
                        self.overlay = Some(ov);
                    }
                    None => {
                        let size = o.image.size;
                        let tex = match self.overlay.take() {
                            Some(mut ov) if ov.size == size => {
                                ov.tex.set(o.image, TextureOptions::LINEAR);
                                ov.tex
                            }
                            _ => ctx.load_texture("live-stroke", o.image, TextureOptions::LINEAR),
                        };
                        self.overlay = Some(Overlay { origin: o.origin, size, tex, fwd: if o.canvas_space { None } else { self.fwd } });
                    }
                }
            }
        }
        if let Some((c, t)) = self.hold {
            if shown_counter.is_some_and(|s| s >= c) || t.elapsed().as_secs_f32() > 5.0 {
                self.overlay = None;
                self.hold = None;
            }
        }
    }

    /// Fim do traço: a thread para e a prévia fica até o render do traço gravado (`counter`).
    pub(crate) fn release(&mut self, counter: Option<u64>) {
        self.stop();
        match counter {
            Some(c) if self.overlay.is_some() => self.hold = Some((c, Instant::now())),
            _ => self.overlay = None,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.shared.is_some()
    }

    /// Borracha escolhida: prepara numa thread a base (quadro abaixo e acima da camada) para `key`, antes do clique,
    /// para o apagar aparecer desde o primeiro movimento. `make` só é chamado quando a chave mudou.
    pub(crate) fn prefetch_erase(&mut self, key: BaseKey, make: impl FnOnce() -> (Project, BlobStore)) {
        if self.prep.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let (project, blobs) = make();
        let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
        let (s2, k2) = (Arc::clone(&slot), key.clone());
        let spawned = std::thread::Builder::new().name("aiai-erase-base".into()).spawn(move || {
            let b = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build_base(&project, &blobs, &k2))).ok().map(Arc::new);
            if let Ok(mut g) = s2.0.lock() {
                *g = Some(b);
            }
            s2.1.notify_all();
        });
        if spawned.is_ok() {
            self.prep = Some((key, slot));
        }
    }

    /// Outra ferramenta: a base da borracha sai da memória.
    pub(crate) fn drop_prefetch(&mut self) {
        self.prep = None;
    }


    fn stop(&mut self) {
        if let Some(sh) = self.shared.take() {
            if let Ok(mut st) = sh.0.lock() {
                st.done = true;
            }
            sh.1.notify_one();
        }
        self.rx = None;
    }
}

/// O que fica na frente da camada: as camadas de cima compostas sobre transparente e os efeitos de cima (pilha e
/// opacidade), na ordem.
struct Above {
    img: Option<aiai_paint::Image>,
    fx: Vec<(Vec<aiai_fx::EffectInstance>, f32)>,
    /// Camada com Recortar: alfa da base do recorte no canvas (o traço só aparece sobre ela, como no render).
    clip: Option<aiai_paint::Image>,
}

fn worker(spec: StartSpec, slot: Option<Slot>, sh: Arc<(Mutex<State>, Condvar)>, tx: crossbeam_channel::Sender<Out>, ctx: egui::Context) {
    // Editar pontos: o quadro sem o traço, sobre o xadrez; pincel: o que fica na frente
    let under = match spec.mode {
        Mode::EditStroke => spec.project.as_ref().and_then(|p| render_without_stroke(p, &spec)),
        _ => None,
    };
    // borracha: o que fica embaixo e em cima da camada (a preparada antes do clique, ou feita agora)
    let base: Option<Arc<EraseBase>> = if erase_mode(spec.mode) {
        slot.and_then(|s| wait_slot(&s)).or_else(|| spec.project.as_ref().map(|p| Arc::new(build_base(p, &spec.blobs, &BaseKey::of(p, &spec)))))
    } else {
        None
    };
    let art = match (spec.mode, spec.project.as_ref(), spec.cel.as_ref()) {
        (Mode::EraseVector | Mode::EraseFill, Some(p), Some(c)) => Some(CelArt::new(p, &spec.blobs, c, &spec.layer)),
        _ => None,
    };
    let above = match spec.mode {
        Mode::Brush | Mode::Pen | Mode::EditStroke => spec.project.as_ref().map(|p| render_above(p, &spec)),
        _ => None,
    };
    // Editar pontos: onde o traço estava (o quadro sem ele aparece ali)
    let old_box = spec.hide.as_ref().and_then(|(_, id)| spec.cel.as_ref()?.strokes.iter().find(|s| &s.id == id)).and_then(|s| stroke_box(s, spec.canvas));
    let mut inc = matches!(spec.mode, Mode::Brush | Mode::EraseRaster).then(|| Incremental::new(&spec));
    loop {
        let req = {
            let Ok(mut st) = sh.0.lock() else { return };
            loop {
                if st.done {
                    return;
                }
                if let Some(r) = st.req.take() {
                    break r;
                }
                st = match sh.1.wait(st) {
                    Ok(s) => s,
                    Err(_) => return,
                };
            }
        };
        let t0 = Instant::now();
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match spec.mode {
            Mode::Pen => pen_overlay(&spec, &req, above.as_ref()),
            Mode::Brush => brush_overlay(&spec, &req, above.as_ref(), inc.as_mut()?),
            Mode::EraseRaster => base.as_ref().and_then(|b| raster_erase_overlay(&spec, &req, b, inc.as_mut()?)),
            Mode::EraseVector => base.as_ref().and_then(|b| vector_erase_overlay(&spec, &req, b, art.as_ref()?)),
            Mode::EraseFill => base.as_ref().and_then(|b| fill_erase_overlay(&spec, &req, b, art.as_ref()?)),
            Mode::EditStroke => under.as_ref().and_then(|u| edit_overlay(&spec, &req, u, above.as_ref(), old_box)),
        }));
        if let Ok(Some(mut o)) = out {
            o.ms = t0.elapsed().as_secs_f32() * 1000.0;
            if tx.send(o).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    }
}

fn px(p: [f32; 4]) -> Color32 {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Color32::from_rgba_premultiplied(b(p[0]), b(p[1]), b(p[2]), b(p[3]))
}

/// Camadas desenháveis do corte de baixo para cima (pastas abertas), com os dados.
pub(crate) fn ordered_leaves(p: &Project, parent: &NodeId, out: &mut Vec<(NodeId, LayerData)>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        match p.node(c).map(|x| &x.data) {
            Some(NodeKind::Layer(l)) if l.kind == LayerType::Group => ordered_leaves(p, c, out),
            Some(NodeKind::Layer(l)) => out.push((c.clone(), l.clone())),
            _ => {}
        }
    }
}

/// Como [`ordered_leaves`], só o que o quadro mostra: camadas visíveis em pastas visíveis. O render desenha mesmo as
/// ocultas quando elas vêm em `only_layers` (linhas de fechamento do balde), então a escolha tem de ser feita aqui:
/// antes uma camada oculta (um fundo branco, por exemplo) aparecia por baixo do que a borracha tirava.
fn visible_leaves(p: &Project, parent: &NodeId, out: &mut Vec<(NodeId, LayerData)>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        match p.node(c).map(|x| &x.data) {
            Some(NodeKind::Layer(l)) if !l.visible => {}
            Some(NodeKind::Layer(l)) if l.kind == LayerType::Group => visible_leaves(p, c, out),
            Some(NodeKind::Layer(l)) => out.push((c.clone(), l.clone())),
            _ => {}
        }
    }
}

/// Camadas visíveis abaixo e acima de `layer` (de baixo para cima); sem ela na lista, tudo fica embaixo.
fn split_at_layer(p: &Project, cut: &NodeId, layer: &NodeId) -> (Vec<(NodeId, LayerData)>, Vec<(NodeId, LayerData)>) {
    let mut all = Vec::new();
    visible_leaves(p, cut, &mut all);
    match all.iter().position(|(id, _)| id == layer) {
        Some(i) => {
            let above = all.split_off(i + 1);
            all.pop();
            (all, above)
        }
        None => (all, Vec::new()),
    }
}

fn render_only(p: &Project, blobs: &BlobStore, cut: &NodeId, frame: u32, guides: bool, reference: bool, ids: Vec<NodeId>) -> Option<aiai_paint::Image> {
    if ids.is_empty() {
        return None;
    }
    let opts = aiai_render::RenderOptions { include_guides: guides, include_reference: reference, only_layers: Some(ids), background: None, camera: false, layer_motion: true };
    aiai_render::render_frame(p, blobs, cut, frame, &opts).ok().map(|o| o.image)
}

/// O que fica na frente: as camadas de cima compostas sobre transparente e os efeitos de cor de cima, na ordem.
fn above_of(p: &Project, blobs: &BlobStore, cut: &NodeId, frame: u32, guides: bool, reference: bool, above: &[(NodeId, LayerData)]) -> Above {
    let ids: Vec<NodeId> = above.iter().filter(|(_, l)| !matches!(l.kind, LayerType::Effect | LayerType::Adjustment)).map(|(id, _)| id.clone()).collect();
    let fx = above
        .iter()
        .filter(|(_, l)| matches!(l.kind, LayerType::Effect | LayerType::Adjustment))
        .filter_map(|(_, l)| aiai_fx::stack_from_params(&l.params).ok().map(|st| (st, l.keys.eval("opacity", frame as f32).unwrap_or(l.opacity))))
        .collect();
    Above { img: render_only(p, blobs, cut, frame, guides, reference, ids), fx, clip: None }
}

fn render_above(p: &Project, spec: &StartSpec) -> Above {
    let (_, above) = split_at_layer(p, &spec.cut, &spec.layer);
    let mut ab = above_of(p, &spec.blobs, &spec.cut, spec.frame, spec.guides, spec.reference, &above);
    ab.clip = super::pen::clip_alpha(p, &spec.blobs, &spec.cut, &spec.layer, spec.frame, spec.guides, spec.reference);
    ab
}

/// Base da borracha: o quadro de baixo e o de cima renderizados em paralelo, e como a camada entra neles.
fn build_base(p: &Project, blobs: &BlobStore, k: &BaseKey) -> EraseBase {
    let (below, above) = split_at_layer(p, &k.cut, &k.layer);
    let ids: Vec<NodeId> = below.iter().map(|(id, _)| id.clone()).collect();
    let (below, above) = std::thread::scope(|s| {
        let h = s.spawn(|| render_only(p, blobs, &k.cut, k.frame, k.guides, k.reference, ids));
        let above = above_of(p, blobs, &k.cut, k.frame, k.guides, k.reference, &above);
        (h.join().ok().flatten(), above)
    });
    let (opacity, blend) = match p.node(&k.layer).map(|n| &n.data) {
        Some(NodeKind::Layer(l)) => {
            let blend = if l.blend == BlendMode::PassThrough { BlendMode::Normal } else { l.blend };
            (l.keys.eval("opacity", k.frame as f32).unwrap_or(l.opacity).clamp(0.0, 1.0), blend)
        }
        _ => (1.0, BlendMode::Normal),
    };
    EraseBase { below, above, opacity, blend }
}

/// Imagem do quadro composta sobre o xadrez do canvas: RGB opaco.
fn over_checker(img: &aiai_paint::Image, spec: &StartSpec) -> (u32, u32, Vec<[f32; 3]>) {
    let ([a, b], cell) = spec.checker;
    let (ca, cb) = (a.to_normalized_gamma_f32(), b.to_normalized_gamma_f32());
    let cell = cell.max(0.5);
    let w = img.width;
    let rgb = img
        .data
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
            let odd = ((x / cell).floor() as i64 + (y / cell).floor() as i64).rem_euclid(2) == 1;
            let c = if odd { cb } else { ca };
            [q[0] + c[0] * (1.0 - q[3]), q[1] + c[1] * (1.0 - q[3]), q[2] + c[2] * (1.0 - q[3])]
        })
        .collect();
    (img.width, img.height, rgb)
}

/// Quadro inteiro sem o traço que está sendo editado, sobre o xadrez (Editar pontos).
fn render_without_stroke(p: &Project, spec: &StartSpec) -> Option<(u32, u32, Vec<[f32; 3]>)> {
    let (cel, id) = spec.hide.as_ref()?;
    let mut q = p.clone();
    if let Some(NodeKind::Cel(c)) = q.node_mut(cel).map(|n| &mut n.data) {
        c.strokes.retain(|s| &s.id != id);
    }
    let opts = aiai_render::RenderOptions { include_guides: spec.guides, include_reference: spec.reference, only_layers: None, background: None, camera: false, layer_motion: true };
    let img = aiai_render::render_frame(&q, &spec.blobs, &spec.cut, spec.frame, &opts).ok()?.image;
    Some(over_checker(&img, spec))
}

/// Caixa (x0, y0, x1, y1) do traço rasterizado, em px do cel.
fn stroke_box(s: &Stroke, canvas: (u32, u32)) -> Option<[i32; 4]> {
    let cov = aiai_paint::stroke_raster::rasterize_stroke(&aiai_paint::sample_stroke(s), s.aliased, canvas.0, canvas.1)?;
    Some([cov.x0, cov.y0, cov.x0 + cov.width as i32, cov.y0 + cov.height as i32])
}

/// Editar pontos: na área do traço antigo e do novo, o quadro sem o traço com o traço novo (motor) por cima, as
/// camadas de cima na frente. Opaco: cobre o traço antigo do quadro mostrado.
fn edit_overlay(spec: &StartSpec, req: &Req, u: &(u32, u32, Vec<[f32; 3]>), above: Option<&Above>, old: Option<[i32; 4]>) -> Option<Out> {
    let new = pen_overlay(spec, req, above);
    let mut b = old.unwrap_or([i32::MAX, i32::MAX, i32::MIN, i32::MIN]);
    if let Some(o) = &new {
        b = [b[0].min(o.origin[0]), b[1].min(o.origin[1]), b[2].max(o.origin[0] + o.image.size[0] as i32), b[3].max(o.origin[1] + o.image.size[1] as i32)];
    }
    let (cw, ch) = (spec.canvas.0 as i32, spec.canvas.1 as i32);
    let (x0, y0, x1, y1) = ((b[0] - 1).max(0), (b[1] - 1).max(0), (b[2] + 1).min(cw), (b[3] + 1).min(ch));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (w, h) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let (px0, py0) = (x0 + x as i32, y0 + y as i32);
            let base = under_at(u, &spec.fwd, px0 as f32 + 0.5, py0 as f32 + 0.5);
            let top = new.as_ref().and_then(|o| {
                let (ox, oy) = (px0 - o.origin[0], py0 - o.origin[1]);
                (ox >= 0 && oy >= 0 && (ox as usize) < o.image.size[0] && (oy as usize) < o.image.size[1]).then(|| o.image.pixels[oy as usize * o.image.size[0] + ox as usize])
            });
            pixels.push(match (base, top) {
                (Some(c), t) => {
                    let t = t.unwrap_or(Color32::TRANSPARENT);
                    let k = 1.0 - t.a() as f32 / 255.0;
                    let f = |v: u8, b: f32| (v as f32 / 255.0 + b * k).min(1.0);
                    px([f(t.r(), c[0]), f(t.g(), c[1]), f(t.b(), c[2]), 1.0])
                }
                (None, Some(t)) => t,
                (None, None) => Color32::TRANSPARENT,
            });
        }
    }
    Some(Out { origin: [x0, y0], image: ColorImage { size: [w, h], source_size: egui::vec2(w as f32, h as f32), pixels }, partial: None, ms: 0.0, canvas_space: false })
}

/// Traço incremental do pincel/borracha raster: os trechos do caminho que já não mudam são carimbados uma vez só;
/// a cada movimento, só o fim (que ainda depende do último ponto) é refeito numa cópia. Mesmo motor e mesma
/// suavização do commit (`paint_on_cel`), então o resultado sai igual, sem o custo crescer com o tamanho do traço.
struct Incremental {
    sb: aiai_paint::brush::StrokeBuffer,
    /// Trechos da spline já carimbados.
    fed: usize,
    /// A camada inteira antes do traço (borrador e recorte do antes/depois).
    layer: aiai_paint::Image,
    /// Retângulo do fim provisório da prévia anterior (é refeito na próxima: o fim muda).
    prev_tail: Option<[i32; 4]>,
}

fn union(a: Option<[i32; 4]>, b: Option<[i32; 4]>) -> Option<[i32; 4]> {
    match (a, b) {
        (Some(a), Some(b)) => Some([a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])]),
        (a, None) => a,
        (None, b) => b,
    }
}

impl Incremental {
    fn new(spec: &StartSpec) -> Self {
        let (w, h) = spec.canvas;
        let layer = spec.cel.as_ref().and_then(|c| aiai_tools::paint::cel_raster(&spec.blobs, c, w, h).ok()).unwrap_or_else(|| aiai_paint::Image::new(w, h));
        Incremental { sb: aiai_paint::brush::StrokeBuffer::new(&spec.preset, spec.color, spec.seed, w, h, [0, 0]), fed: 0, layer, prev_tail: None }
    }

    fn region(&mut self, spec: &StartSpec, req: &Req) -> Option<aiai_tools::paint::PaintedRegion> {
        let pts: Vec<BrushPoint> = req.pts.iter().filter_map(|q| match q.as_slice() {
            [x, y] => Some(BrushPoint { x: *x, y: *y, pressure: 1.0 }),
            [x, y, pr] | [x, y, pr, _] => Some(BrushPoint { x: *x, y: *y, pressure: pr.clamp(0.0, 1.0) }),
            _ => None,
        }).collect();
        let n = pts.len();
        if n == 0 {
            return None;
        }
        // com o tempo de cada ponto (o mesmo do commit), o pincel vê a velocidade
        let times: Option<Vec<f32>> = (req.pts.len() == n && req.pts.iter().all(|q| q.len() == 4 && q[3].is_finite())).then(|| req.pts.iter().map(|q| q[3]).collect());
        let smudge = spec.preset.reads_layer().then_some(&self.layer);
        let feed = |sb: &mut aiai_paint::brush::StrokeBuffer, i: usize| match &times {
            Some(ts) => {
                let (seg, st) = aiai_tools::paint::spline_segment_timed(&pts, ts, i);
                sb.add_timed(smudge, &seg, &st);
            }
            None => sb.add(smudge, &aiai_tools::paint::spline_segment(&pts, i)),
        };
        // o último ponto ainda pode mudar (espaçamento das amostras): pronto é o trecho que não depende dele
        while n >= 4 && self.fed + 4 <= n {
            feed(&mut self.sb, self.fed);
            self.fed += 1;
        }
        let committed = self.sb.take_dirty();
        let mut tmp = self.sb.clone();
        let tail = |sb: &mut aiai_paint::brush::StrokeBuffer, from: usize| match &times {
            Some(ts) => sb.add_timed(smudge, &pts[from..], &ts[from..]),
            None => sb.add(smudge, &pts[from..]),
        };
        if n < 3 {
            tail(&mut tmp, 0);
        } else {
            for i in self.fed..n - 1 {
                feed(&mut tmp, i);
            }
            tail(&mut tmp, n - 1);
        }
        tmp.finish(smudge);
        // só o que mudou: os trechos novos, o fim novo e o fim anterior (que pode ter recuado)
        let tail = tmp.take_dirty();
        let changed = union(union(committed, tail), self.prev_tail);
        self.prev_tail = tail;
        let b = changed?;
        let m = if spec.preset.wet_edges > 0.0 { 12 } else { 0 };
        let (w, h) = (self.layer.width as i32, self.layer.height as i32);
        let area = [(b[0] - m).max(0), (b[1] - m).max(0), (b[2] + m).min(w), (b[3] + m).min(h)];
        if area[0] >= area[2] || area[1] >= area[3] {
            return None;
        }
        let (cw, ch) = ((area[2] - area[0]) as u32, (area[3] - area[1]) as u32);
        let mut crop = aiai_paint::Image::new(cw, ch);
        for y in 0..ch {
            let src = self.layer.idx(area[0] as u32, area[1] as u32 + y);
            let dst = crop.idx(0, y);
            crop.data[dst..dst + cw as usize].copy_from_slice(&self.layer.data[src..src + cw as usize]);
        }
        let before = crop.clone();
        tmp.composite_rect(&mut crop, [area[0], area[1]], area);
        Some(aiai_tools::paint::PaintedRegion { origin: [area[0], area[1]], before: Some(before), after: crop, bounds: area })
    }
}

/// Recorte [x0, y0, x1, y1) relativo ao `origin` da região tocada.
fn crop_of(r: &aiai_tools::paint::PaintedRegion) -> (usize, usize, usize, usize) {
    let (w, h) = (r.after.width as i32, r.after.height as i32);
    let x0 = (r.bounds[0] - r.origin[0]).clamp(0, w) as usize;
    let y0 = (r.bounds[1] - r.origin[1]).clamp(0, h) as usize;
    let x1 = (r.bounds[2] - r.origin[0]).clamp(0, w) as usize;
    let y1 = (r.bounds[3] - r.origin[1]).clamp(0, h) as usize;
    (x0, y0, x1, y1)
}

fn brush_overlay(spec: &StartSpec, req: &Req, above: Option<&Above>, inc: &mut Incremental) -> Option<Out> {
    let mut r = inc.region(spec, req)?;
    if let Some(m) = &spec.clip {
        aiai_tools::paint::clip_region(&mut r, m, spec.fwd.as_ref());
    }
    let before = r.before.as_ref()?;
    let (x0, y0, x1, y1) = crop_of(&r);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let w = r.after.width as usize;
    let (cw, ch) = (x1 - x0, y1 - y0);
    let k = spec.layer_opacity.clamp(0.0, 1.0);
    let mut crop = aiai_paint::Image::new(cw as u32, ch as u32);
    let mut mask = vec![false; cw * ch];
    for y in y0..y1 {
        for x in x0..x1 {
            let (a, b) = (r.after.data[y * w + x], before.data[y * w + x]);
            if !(0..4).any(|c| (a[c] - b[c]).abs() > 1e-4) {
                continue;
            }
            let i = (y - y0) * cw + (x - x0);
            crop.data[i] = [a[0] * k, a[1] * k, a[2] * k, a[3] * k];
            mask[i] = true;
        }
    }
    let mut o = dress(spec, [r.origin[0] + x0 as i32, r.origin[1] + y0 as i32], crop, &mask, above);
    o.partial = Some([spec.canvas.0 as usize, spec.canvas.1 as usize]);
    Some(o)
}

/// Caneta: o traço rasterizado como o render faz (`sample_stroke` + `rasterize_stroke`), na cor e opacidade dele.
fn pen_overlay(spec: &StartSpec, req: &Req, above: Option<&Above>) -> Option<Out> {
    let s = req.stroke.as_ref()?;
    let smp = aiai_paint::sample_stroke(s);
    let cov = aiai_paint::stroke_raster::rasterize_stroke(&smp, s.aliased, spec.canvas.0, spec.canvas.1)?;
    let (cw, ch) = (cov.width as usize, cov.height as usize);
    if cw == 0 || ch == 0 {
        return None;
    }
    let a = spec.color[3] * s.opacity * spec.layer_opacity.clamp(0.0, 1.0);
    let mut crop = aiai_paint::Image::new(cw as u32, ch as u32);
    let mut mask = vec![false; cw * ch];
    for (i, c) in cov.cov.iter().enumerate() {
        if *c > 0.0 {
            let k = a * c;
            crop.data[i] = [spec.color[0] * k, spec.color[1] * k, spec.color[2] * k, k];
            mask[i] = true;
        }
    }
    Some(dress(spec, [cov.x0, cov.y0], crop, &mask, above))
}

/// O que fica na frente (camadas de cima, efeitos de cor de cima) aplicado aos pixels do traço (`mask`).
fn dress(spec: &StartSpec, origin: [i32; 2], crop: aiai_paint::Image, mask: &[bool], above: Option<&Above>) -> Out {
    dress_at(spec, spec.fwd.as_ref(), origin, crop, mask, above)
}

/// Como [`dress`], com `fwd` = pixel da área → canvas (`None` = a área já está no canvas).
fn dress_at(spec: &StartSpec, fwd: Option<&Affine>, origin: [i32; 2], mut crop: aiai_paint::Image, mask: &[bool], above: Option<&Above>) -> Out {
    let (cw, ch) = (crop.width as usize, crop.height as usize);
    // camada com Recortar: o traço só aparece sobre o desenho da base do recorte (como o render vai mostrar)
    if let Some(base) = above.and_then(|ab| ab.clip.as_ref()) {
        super::pen::clip_pixels(&mut crop, mask, origin, fwd, base);
    }
    // as camadas de cima ficam na frente do traço (a cerca na frente da casa)
    if let Some(img) = above.and_then(|ab| ab.img.as_ref()) {
        for y in 0..ch {
            for x in 0..cw {
                let i = y * cw + x;
                if !mask[i] {
                    continue;
                }
                let (px0, py0) = ((origin[0] + x as i32) as f32 + 0.5, (origin[1] + y as i32) as f32 + 0.5);
                let (cx, cy) = match fwd {
                    Some(af) => af.apply(px0, py0),
                    None => (px0, py0),
                };
                let (xi, yi) = (cx.floor() as i64, cy.floor() as i64);
                if xi >= 0 && yi >= 0 && (xi as u32) < img.width && (yi as u32) < img.height {
                    let t = img.data[(yi as u32 * img.width + xi as u32) as usize];
                    let o = crop.data[i];
                    crop.data[i] = [t[0] + o[0] * (1.0 - t[3]), t[1] + o[1] * (1.0 - t[3]), t[2] + o[2] * (1.0 - t[3]), t[3] + o[3] * (1.0 - t[3])];
                }
            }
        }
    }
    // efeitos de cor de cima (Grade): o que é feito em cada pixel sai igual; efeitos espaciais ficam aproximados
    if let Some(ab) = above {
        for (stack, opacity) in &ab.fx {
            let before_fx = crop.clone();
            let mut done = crop.clone();
            let ctx = aiai_fx::FxContext { frame: spec.frame, below: Some(&before_fx), blobs: &spec.blobs, seed: 0 };
            let _ = aiai_fx::apply_stack(&mut done, stack, &ctx);
            let k = opacity.clamp(0.0, 1.0);
            for (d, (a, b)) in crop.data.iter_mut().zip(before_fx.data.iter().zip(&done.data)) {
                for c in 0..4 {
                    d[c] = a[c] + (b[c] - a[c]) * k;
                }
            }
        }
    }
    let pixels = crop.data.iter().map(|p| px(*p)).collect();
    Out { origin, image: ColorImage { size: [cw, ch], source_size: egui::vec2(cw as f32, ch as f32), pixels }, partial: None, ms: 0.0, canvas_space: false }
}

/// Pixel do "embaixo" (canvas) para o pixel (x, y) do cel.
fn under_at(u: &(u32, u32, Vec<[f32; 3]>), fwd: &Option<Affine>, x: f32, y: f32) -> Option<[f32; 3]> {
    let (cx, cy) = match fwd {
        Some(a) => a.apply(x, y),
        None => (x, y),
    };
    let (xi, yi) = (cx.floor() as i64, cy.floor() as i64);
    if xi < 0 || yi < 0 || xi >= u.0 as i64 || yi >= u.1 as i64 {
        return None;
    }
    Some(u.2[(yi as u32 * u.0 + xi as u32) as usize])
}

/// Pixel do canvas embaixo da camada (canvas px `cx`, `cy`): o quadro de baixo sobre o xadrez, opaco.
fn under_pixel(spec: &StartSpec, base: &EraseBase, cx: f32, cy: f32) -> [f32; 4] {
    let ([a, b], cell) = spec.checker;
    let cell = cell.max(0.5);
    let odd = ((cx / cell).floor() as i64 + (cy / cell).floor() as i64).rem_euclid(2) == 1;
    let c = if odd { b } else { a }.to_normalized_gamma_f32();
    let (xi, yi) = (cx.floor() as i64, cy.floor() as i64);
    let q = base
        .below
        .as_ref()
        .filter(|img| xi >= 0 && yi >= 0 && xi < img.width as i64 && yi < img.height as i64)
        .map(|img| img.data[(yi as u32 * img.width + xi as u32) as usize])
        .unwrap_or([0.0; 4]);
    [q[0] + c[0] * (1.0 - q[3]), q[1] + c[1] * (1.0 - q[3]), q[2] + c[2] * (1.0 - q[3]), 1.0]
}

/// Recompõe a área `w`×`h` em `origin` (px do cel): onde a camada mudou (`before` → `after`, pré-multiplicados), o
/// quadro de baixo sobre o xadrez, a camada depois do apagar com a opacidade e o modo dela e o que fica na frente.
/// Opaco onde mudou, transparente no resto (ali o quadro mostrado já está certo).
fn erase_result(spec: &StartSpec, base: &EraseBase, fwd: Option<&Affine>, origin: [i32; 2], w: usize, h: usize, before: impl Fn(usize, usize) -> [f32; 4], after: impl Fn(usize, usize) -> [f32; 4]) -> Out {
    let mut crop = aiai_paint::Image::new(w as u32, h as u32);
    let mut changed = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (after(x, y), before(x, y));
            changed[y * w + x] = (0..4).any(|c| (a[c] - b[c]).abs() > 1e-5);
        }
    }
    // mais 1 px em volta (ali a recomposição dá o próprio quadro): a textura é amostrada com filtro linear e, fora
    // de 100%, a borda opaca da prévia se misturaria com o quadro antigo (um fio do traço apagado aparecia na borda)
    let mut mask = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let (ya, yb, xa, xb) = (y.saturating_sub(1), (y + 1).min(h - 1), x.saturating_sub(1), (x + 1).min(w - 1));
            mask[y * w + x] = (ya..=yb).any(|yy| (xa..=xb).any(|xx| changed[yy * w + xx]));
        }
    }
    for y in 0..h {
        for x in 0..w {
            if !mask[y * w + x] {
                continue;
            }
            let a = after(x, y);
            let (px0, py0) = ((origin[0] + x as i32) as f32 + 0.5, (origin[1] + y as i32) as f32 + 0.5);
            let (cx, cy) = match fwd {
                Some(af) => af.apply(px0, py0),
                None => (px0, py0),
            };
            let mut d = under_pixel(spec, base, cx, cy);
            aiai_paint::blend::blend_pixel(&mut d, a, base.blend, base.opacity);
            let i = y * w + x;
            crop.data[i] = d;
        }
    }
    let mut o = dress_at(spec, fwd, origin, crop, &mask, Some(&base.above));
    o.canvas_space = fwd.is_none() && spec.fwd.is_some();
    for (p, m) in o.image.pixels.iter_mut().zip(&mask) {
        if !*m {
            *p = Color32::TRANSPARENT;
        }
    }
    o
}

fn raster_erase_overlay(spec: &StartSpec, req: &Req, base: &EraseBase, inc: &mut Incremental) -> Option<Out> {
    let mut r = inc.region(spec, req)?;
    if let Some(m) = &spec.clip {
        aiai_tools::paint::clip_region(&mut r, m, spec.fwd.as_ref());
    }
    let before = r.before.as_ref()?;
    let (x0, y0, x1, y1) = crop_of(&r);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let w = r.after.width as usize;
    let at = |img: &aiai_paint::Image, x: usize, y: usize| img.data[(y0 + y) * w + x0 + x];
    let mut o = erase_result(spec, base, spec.fwd.as_ref(), [r.origin[0] + x0 as i32, r.origin[1] + y0 as i32], x1 - x0, y1 - y0, |x, y| at(before, x, y), |x, y| at(&r.after, x, y));
    o.partial = Some([spec.canvas.0 as usize, spec.canvas.1 as usize]);
    Some(o)
}

/// Região de preenchimento do cel com a máscara já lida (uma vez, no começo do traço).
struct ArtFill {
    /// [x, y, largura, altura] em px do cel.
    bounds: [i32; 4],
    premul: [f32; 4],
    mask: Option<aiai_paint::Mask>,
    /// Degradê da região (como o render: na caixa da máscara).
    grad: Option<(aiai_core::ResolvedGradient, aiai_core::paint::GradientMapper)>,
}

impl ArtFill {
    /// O ponto cai na região (o mesmo teste da borracha em camada de preenchimento).
    fn covers(&self, x: f32, y: f32) -> bool {
        let [bx, by, bw, bh] = self.bounds;
        let (xi, yi) = (x.floor() as i32, y.floor() as i32);
        if xi < bx || yi < by || xi >= bx + bw || yi >= by + bh {
            return false;
        }
        self.mask.as_ref().is_some_and(|m| m.get((xi - bx) as u32, (yi - by) as u32) > 0.5)
    }
}

/// Cel vetorial/de preenchimento desenhado por partes como o render desenha (`render_cel`: preenchimentos e depois
/// os traços, na ordem, com a cor resolvida pela paleta).
struct CelArt {
    fills: Vec<ArtFill>,
    hide_trace: bool,
}

fn resolve_rgba(p: &Project, c: &ColorRef) -> [f32; 4] {
    match c {
        ColorRef::Direct { rgba } => rgba.0,
        ColorRef::Palette { palette, color } => match p.node(palette).map(|n| &n.data) {
            Some(NodeKind::Palette(pd)) => pd.resolve(color).map(|c| c.0).unwrap_or([1.0, 0.0, 1.0, 1.0]),
            _ => [1.0, 0.0, 1.0, 1.0],
        },
    }
}

impl CelArt {
    fn new(p: &Project, blobs: &BlobStore, cel: &CelData, layer: &NodeId) -> CelArt {
        let fills = cel
            .fills
            .iter()
            .map(|f| {
                let c = resolve_rgba(p, &f.color);
                let mask = f.mask.as_ref().and_then(|h| blobs.get(h)).and_then(|b| aiai_paint::Mask::decode(&b).ok());
                let grad = f.gradient.as_ref().map(|g| {
                    let r = g.resolve(|c| resolve_rgba(p, c));
                    let [bx, by, bw, bh] = f.bounds;
                    let m = r.mapper([bx as f32, by as f32, (bx + bw) as f32, (by + bh) as f32]);
                    (r, m)
                });
                ArtFill { bounds: f.bounds, premul: [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]], mask, grad }
            })
            .collect();
        let hide_trace = match p.node(layer).map(|n| &n.data) {
            Some(NodeKind::Layer(l)) => l.params.get("hide_color_trace").and_then(|v| v.as_bool()).unwrap_or(false),
            _ => false,
        };
        CelArt { fills, hide_trace }
    }

    /// A caixa `b` = [x0, y0, x1, y1) do cel com as regiões `keep` e os traços `strokes` (canvas `canvas`). Com
    /// `xf`, só traços (cel só de vetor numa camada com movimento): cada amostra vai para o canvas e o raio escala,
    /// como o render faz (a caixa então é do canvas).
    fn draw(&self, p: &Project, keep: impl Fn(usize) -> bool, strokes: &[&Stroke], b: [i32; 4], canvas: (u32, u32)) -> aiai_paint::Image {
        self.draw_xf(p, keep, strokes, b, canvas, None)
    }

    fn draw_xf(&self, p: &Project, keep: impl Fn(usize) -> bool, strokes: &[&Stroke], b: [i32; 4], canvas: (u32, u32), xf: Option<&Affine>) -> aiai_paint::Image {
        let (bw, bh) = ((b[2] - b[0]).max(0) as u32, (b[3] - b[1]).max(0) as u32);
        let mut img = aiai_paint::Image::new(bw, bh);
        for (i, f) in self.fills.iter().enumerate() {
            let Some(m) = f.mask.as_ref().filter(|_| keep(i)) else { continue };
            let (fx, fy) = (f.bounds[0], f.bounds[1]);
            for y in b[1].max(fy)..b[3].min(fy + m.height as i32) {
                for x in b[0].max(fx)..b[2].min(fx + m.width as i32) {
                    let cov = m.get((x - fx) as u32, (y - fy) as u32);
                    if cov > 0.0 {
                        let i = img.idx((x - b[0]) as u32, (y - b[1]) as u32);
                        let src = match &f.grad {
                            Some((g, gm)) => g.sample_premul(gm, [x as f32 + 0.5, y as f32 + 0.5]),
                            None => f.premul,
                        };
                        aiai_paint::Image::over(&mut img.data[i], src, cov);
                    }
                }
            }
        }
        for s in strokes {
            if self.hide_trace && s.color_trace {
                continue;
            }
            let mut smp = aiai_paint::sample_stroke(s);
            if let Some(a) = xf {
                let k = (a.0[0] * a.0[4] - a.0[1] * a.0[3]).abs().sqrt();
                for q in &mut smp {
                    (q.x, q.y) = a.apply(q.x, q.y);
                    q.r *= k;
                }
            }
            // fora da caixa: nem rasteriza
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for q in &smp {
                (x0, y0, x1, y1) = (x0.min(q.x - q.r), y0.min(q.y - q.r), x1.max(q.x + q.r), y1.max(q.y + q.r));
            }
            if x1 + 2.0 < b[0] as f32 || y1 + 2.0 < b[1] as f32 || x0 - 2.0 > b[2] as f32 || y0 - 2.0 > b[3] as f32 {
                continue;
            }
            let Some(cov) = aiai_paint::stroke_raster::rasterize_stroke(&smp, s.aliased, canvas.0, canvas.1) else { continue };
            let color = resolve_rgba(p, &s.color);
            let a = color[3] * s.opacity;
            let premul = [color[0] * a, color[1] * a, color[2] * a, a];
            let (cx0, cy0) = (cov.x0, cov.y0);
            for y in b[1].max(cy0)..b[3].min(cy0 + cov.height as i32) {
                for x in b[0].max(cx0)..b[2].min(cx0 + cov.width as i32) {
                    let c = cov.cov[((y - cy0) as u32 * cov.width + (x - cx0) as u32) as usize];
                    if c > 0.0 {
                        let i = img.idx((x - b[0]) as u32, (y - b[1]) as u32);
                        aiai_paint::Image::over(&mut img.data[i], premul, c);
                    }
                }
            }
        }
        img
    }
}

/// Caixa com 1 px de folga presa ao canvas; `None` se vazia.
fn pad_box(b: [i32; 4], canvas: (u32, u32)) -> Option<[i32; 4]> {
    let b = [(b[0] - 1).max(0), (b[1] - 1).max(0), (b[2] + 1).min(canvas.0 as i32), (b[3] + 1).min(canvas.1 as i32)];
    (b[0] < b[2] && b[1] < b[3]).then_some(b)
}

/// Antes e depois desenhados na caixa (px do cel) e recompostos. Camada com movimento: no canvas, como o render a
/// coloca — traços transformados um a um (cel só de vetor) ou o cel inteiro reamostrado — para a borda da prévia
/// ser a mesma do quadro (no espaço do cel a textura esticada deixava um fio do traço apagado).
fn art_result(spec: &StartSpec, base: &EraseBase, art: &CelArt, b: [i32; 4], keep_before: impl Fn(usize) -> bool, keep_after: impl Fn(usize) -> bool, before: &[&Stroke], after: &[&Stroke]) -> Option<Out> {
    let p = spec.project.as_ref()?;
    let cv = spec.canvas;
    let (bi, ai) = match spec.fwd.as_ref() {
        None => (art.draw(p, keep_before, before, b, cv), art.draw(p, keep_after, after, b, cv)),
        Some(af) => {
            // caixa do canvas: os cantos da caixa do cel levados pela transformação, com folga
            let cs = [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])].map(|(x, y)| af.apply(x as f32, y as f32));
            let (x0, y0) = cs.iter().fold((f32::MAX, f32::MAX), |(a, c), q| (a.min(q.0), c.min(q.1)));
            let (x1, y1) = cs.iter().fold((f32::MIN, f32::MIN), |(a, c), q| (a.max(q.0), c.max(q.1)));
            let cb = pad_box([x0.floor() as i32 - 1, y0.floor() as i32 - 1, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1], cv)?;
            let only_strokes = art.fills.is_empty();
            let (bi, ai) = if only_strokes {
                (art.draw_xf(p, |_| true, before, cb, cv, Some(af)), art.draw_xf(p, |_| true, after, cb, cv, Some(af)))
            } else {
                let whole = [0, 0, cv.0 as i32, cv.1 as i32];
                let placed = |img: aiai_paint::Image| crop_image(&aiai_paint::transform_image(&img, *af, cv.0, cv.1), cb);
                (placed(art.draw(p, keep_before, before, whole, cv)), placed(art.draw(p, keep_after, after, whole, cv)))
            };
            let w = bi.width as usize;
            return Some(erase_result(spec, base, None, [cb[0], cb[1]], w, bi.height as usize, |x, y| bi.data[y * w + x], |x, y| ai.data[y * w + x]));
        }
    };
    let w = bi.width as usize;
    Some(erase_result(spec, base, None, [b[0], b[1]], w, bi.height as usize, |x, y| bi.data[y * w + x], |x, y| ai.data[y * w + x]))
}

fn crop_image(img: &aiai_paint::Image, b: [i32; 4]) -> aiai_paint::Image {
    let (w, h) = ((b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
    let mut out = aiai_paint::Image::new(w, h);
    for y in 0..h {
        let src = img.idx(b[0] as u32, b[1] as u32 + y);
        let dst = out.idx(0, y);
        out.data[dst..dst + w as usize].copy_from_slice(&img.data[src..src + w as usize]);
    }
    out
}

/// Borracha vetorial: o comando do commit (`erase_vector_command`) aplicado aos traços do cel — sem os removidos, com
/// os pedaços que ficam nos lugares da pilha — e a área dos traços tocados redesenhada antes e depois.
fn vector_erase_overlay(spec: &StartSpec, req: &Req, base: &EraseBase, art: &CelArt) -> Option<Out> {
    let p = spec.project.as_ref()?;
    if req.path.is_empty() {
        return None;
    }
    let er = aiai_tools::erase::EraseVectorRequest { cut: spec.cut.clone(), frame: spec.frame, layer: spec.layer.clone(), path: req.path.clone(), radius: spec.erase_radius, mode: spec.erase_mode };
    let Command::Batch { commands, .. } = aiai_tools::erase::erase_vector_command(p, &er).ok()? else { return None };
    let cel = spec.cel.as_ref()?;
    let (mut removed, mut inserted): (Vec<Id>, Vec<&aiai_core::command::IndexedStroke>) = (Vec::new(), Vec::new());
    for c in &commands {
        match c {
            Command::RemoveStrokes { ids, .. } => removed.extend(ids.iter().cloned()),
            Command::InsertStrokes { items, .. } => inserted.extend(items.iter()),
            _ => {}
        }
    }
    let mut b: Option<[i32; 4]> = None;
    for s in cel.strokes.iter().filter(|s| removed.contains(&s.id)) {
        b = union(b, stroke_box(s, spec.canvas));
    }
    let b = pad_box(b?, spec.canvas)?;
    let before: Vec<&Stroke> = cel.strokes.iter().collect();
    let mut after: Vec<&Stroke> = cel.strokes.iter().filter(|s| !removed.contains(&s.id)).collect();
    inserted.sort_by_key(|it| it.index);
    for it in inserted {
        let i = it.index.min(after.len());
        after.insert(i, &it.stroke);
    }
    art_result(spec, base, art, b, |_| true, |_| true, &before, &after)
}

/// Borracha em camada de preenchimento: as regiões em que o caminho passa saem (o mesmo teste do commit).
fn fill_erase_overlay(spec: &StartSpec, req: &Req, base: &EraseBase, art: &CelArt) -> Option<Out> {
    let cel = spec.cel.as_ref()?;
    let gone: Vec<bool> = art.fills.iter().map(|f| req.path.iter().any(|q| f.covers(q[0], q[1]))).collect();
    let mut b: Option<[i32; 4]> = None;
    for (f, _) in art.fills.iter().zip(&gone).filter(|(_, g)| **g) {
        b = union(b, Some([f.bounds[0], f.bounds[1], f.bounds[0] + f.bounds[2], f.bounds[1] + f.bounds[3]]));
    }
    let b = pad_box(b?, spec.canvas)?;
    let strokes: Vec<&Stroke> = cel.strokes.iter().collect();
    art_result(spec, base, art, b, |_| true, |i| !gone[i], &strokes, &strokes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::commands::{self as cmds, add_layer, quick_start};
    use aiai_core::command::IndexedStroke;
    use aiai_tools::paint as tpaint;

    const CHECKER: [Color32; 2] = [Color32::from_rgb(0xe6, 0xe7, 0xea), Color32::from_rgb(0xcf, 0xd0, 0xd5)];

    /// O quadro como o canvas mostra: o render (sem câmera) sobre o xadrez, em 8 bits.
    fn shown(core: &CoreSession, cut: &NodeId) -> Vec<Color32> {
        let opts = aiai_render::RenderOptions { include_guides: false, include_reference: false, only_layers: None, background: None, camera: false, layer_motion: true };
        let img = aiai_render::render_frame(&core.project, &core.blobs, cut, 0, &opts).unwrap().image;
        let (ca, cb) = (CHECKER[0].to_normalized_gamma_f32(), CHECKER[1].to_normalized_gamma_f32());
        img.data
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let (x, y) = ((i as u32 % img.width) as f32 + 0.5, (i as u32 / img.width) as f32 + 0.5);
                let odd = ((x / 8.0).floor() as i64 + (y / 8.0).floor() as i64).rem_euclid(2) == 1;
                let c = if odd { cb } else { ca };
                px([q[0] + c[0] * (1.0 - q[3]), q[1] + c[1] * (1.0 - q[3]), q[2] + c[2] * (1.0 - q[3]), 1.0])
            })
            .collect()
    }

    /// A prévia por cima do quadro mostrado antes (como o canvas desenha o quad da prévia).
    fn with_overlay(before: &[Color32], w: usize, o: &Out) -> Vec<Color32> {
        let mut out = before.to_vec();
        let [ow, oh] = o.image.size;
        for y in 0..oh {
            for x in 0..ow {
                let (cx, cy) = (o.origin[0] + x as i32, o.origin[1] + y as i32);
                if cx < 0 || cy < 0 || cx as usize >= w || cy as usize >= out.len() / w {
                    continue;
                }
                let t = o.image.pixels[y * ow + x];
                let d = &mut out[cy as usize * w + cx as usize];
                let k = 1.0 - t.a() as f32 / 255.0;
                let f = |a: u8, b: u8| (a as f32 + b as f32 * k).round().min(255.0) as u8;
                *d = Color32::from_rgba_premultiplied(f(t.r(), d.r()), f(t.g(), d.g()), f(t.b(), d.b()), 255);
            }
        }
        out
    }

    fn assert_same(live: &[Color32], commit: &[Color32], w: usize, what: &str) {
        let mut worst = (0u8, 0usize);
        for (i, (a, b)) in live.iter().zip(commit).enumerate() {
            let d = [a.r().abs_diff(b.r()), a.g().abs_diff(b.g()), a.b().abs_diff(b.b())].into_iter().max().unwrap();
            if d > worst.0 {
                worst = (d, i);
            }
        }
        assert!(worst.0 <= 2, "{what}: a prévia difere do commit em ({}, {}): prévia {:?} × commit {:?}", worst.1 % w, worst.1 / w, live[worst.1], commit[worst.1]);
    }

    fn paint(core: &mut CoreSession, cut: &NodeId, layer: &NodeId, preset: aiai_paint::brush::BrushPreset, rgba: [u8; 4], pts: Vec<Vec<f32>>) {
        let mut blobs = core.blobs.clone();
        let req = tpaint::PaintRequest { cut: cut.clone(), frame: 0, layer: layer.clone(), preset, color: ColorRef::Direct { rgba: Rgba([rgba[0] as f32 / 255.0, rgba[1] as f32 / 255.0, rgba[2] as f32 / 255.0, rgba[3] as f32 / 255.0]) }, strokes: vec![tpaint::PaintStrokeSpec { points: pts, smooth: true }], seed: 7 };
        let cmd = tpaint::paint_command_in(&core.project, &mut blobs, &req, None).unwrap();
        core.blobs = blobs;
        cmds::run(core, cmd).unwrap();
    }

    fn line(y: f32, x0: f32, x1: f32, w: f32, rgb: [u8; 3]) -> Stroke {
        let sp = |x: f32| StrokePoint { x, y, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None };
        Stroke {
            id: Id(String::new()),
            points: vec![sp(x0), sp((x0 + x1) / 2.0), sp(x1)],
            geometry: StrokeGeometry::Polyline,
            tension: 0.0,
            tip: VectorTip::Marker,
            width: w,
            profile: WidthProfile::default(),
            color: ColorRef::Direct { rgba: Rgba::rgb8(rgb[0], rgb[1], rgb[2]) },
            opacity: 1.0,
            closed: false,
            aliased: false,
            color_trace: false,
        }
    }

    /// Fundo raster opaco embaixo, a camada apagada no meio e uma linha vetorial na frente.
    fn doc(target: LayerType) -> (CoreSession, NodeId, NodeId) {
        let mut core = CoreSession::for_tests("borracha");
        core.project.settings.width = 96;
        core.project.settings.height = 64;
        let cut = quick_start(&mut core, 4, None).unwrap();
        let bg = add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let mid = add_layer(&mut core, &cut, target, None).unwrap();
        let top = add_layer(&mut core, &cut, LayerType::Vector, None).unwrap();
        let mut all = Vec::new();
        ordered_leaves(&core.project, &cut, &mut all);
        let pos = |id: &NodeId| all.iter().position(|(i, _)| i == id).unwrap();
        assert!(pos(&bg) < pos(&mid) && pos(&mid) < pos(&top), "ordem de baixo para cima");
        // fundo cobre só a metade de cima (embaixo fica o xadrez)
        let mut big = aiai_paint::brush::BrushPreset::round("fundo", 40.0, 1.0);
        big.spacing = 0.05;
        paint(&mut core, &cut, &bg, big, [200, 40, 30, 255], vec![vec![-10.0, 12.0, 1.0], vec![110.0, 12.0, 1.0]]);
        let cel = cmds::exposed_cel(&core.project, &cut, &top, 0).unwrap();
        cmds::run(&mut core, Command::InsertStrokes { cel, items: vec![IndexedStroke { index: 0, stroke: line(30.0, 40.0, 56.0, 3.0, [20, 20, 160]) }] }).unwrap();
        (core, cut, mid)
    }

    fn spec(core: &CoreSession, cut: &NodeId, layer: &NodeId, mode: Mode, preset: aiai_paint::brush::BrushPreset, erase_mode: EraseMode, radius: f32) -> StartSpec {
        let cel = cmds::exposed_cel(&core.project, cut, layer, 0).and_then(|c| cmds::cel_data(&core.project, &c).cloned());
        let l = cmds::layer_data(&core.project, layer).unwrap().clone();
        StartSpec {
            mode,
            project: Some(core.project.clone()),
            blobs: core.blobs.clone(),
            cut: cut.clone(),
            layer: layer.clone(),
            frame: 0,
            cel,
            canvas: cmds::canvas_size(&core.project, cut),
            preset,
            color: [0.0, 0.0, 0.0, 1.0],
            seed: 7,
            layer_opacity: l.opacity,
            fwd: cmds::layer_placement(&core.project, cut, layer, 0),
            guides: false,
            reference: false,
            checker: (CHECKER, 8.0),
            erase_radius: radius,
            erase_mode,
            hide: None,
            clip: None,
        }
    }

    fn eraser(hardness: f32, size: f32) -> aiai_paint::brush::BrushPreset {
        let o = crate::app::editor::ToolOptions { eraser_size: size, eraser_hardness: hardness, ..Default::default() };
        crate::app::tools::eraser_preset(&o, None)
    }

    /// Raster: a prévia ao vivo por cima do quadro dá exatamente o quadro depois do commit (camada com partes
    /// semitransparentes, opacidade de camada, fundo embaixo, xadrez onde não há nada, linha vetorial na frente).
    #[test]
    fn live_raster_erase_equals_commit() {
        for hardness in [1.0, 0.5, 0.0] {
            let (mut core, cut, mid) = doc(LayerType::Raster);
            let mut solid = aiai_paint::brush::BrushPreset::round("tinta", 14.0, 1.0);
            solid.spacing = 0.05;
            paint(&mut core, &cut, &mid, solid.clone(), [30, 160, 60, 255], vec![vec![10.0, 30.0, 1.0], vec![86.0, 30.0, 1.0]]);
            paint(&mut core, &cut, &mid, solid, [240, 220, 40, 120], vec![vec![10.0, 46.0, 1.0], vec![86.0, 46.0, 1.0]]);
            cmds::set_layer_props(&mut core, &mid, serde_json::json!({"opacity": 0.8})).unwrap();
            let w = core.project.settings.width as usize;
            let before = shown(&core, &cut);
            let pre = eraser(hardness, 12.0);
            let pts: Vec<Vec<f32>> = (0..=24).map(|i| vec![48.0 + 3.0 * (i as f32 * 0.4).sin(), 4.0 + i as f32 * 2.4, 1.0]).collect();
            let sp = spec(&core, &cut, &mid, Mode::EraseRaster, pre.clone(), EraseMode::Segment, 6.0);
            let base = build_base(sp.project.as_ref().unwrap(), &sp.blobs, &BaseKey::of(sp.project.as_ref().unwrap(), &sp));
            let mut inc = Incremental::new(&sp);
            // o traço chega aos poucos, como no arrasto; a prévia é uma textura do canvas com pedaços atualizados
            let mut live = before.clone();
            for n in [3usize, 9, 17, pts.len()] {
                let req = Req { pts: pts[..n].to_vec(), path: Vec::new(), stroke: None };
                if let Some(o) = raster_erase_overlay(&sp, &req, &base, &mut inc) {
                    // um pedaço refeito substitui o que havia nele (a textura parcial), sobre o quadro de antes
                    let fresh = with_overlay(&before, w, &o);
                    let [ow, oh] = o.image.size;
                    for y in 0..oh {
                        for x in 0..ow {
                            let i = (o.origin[1] as usize + y) * w + o.origin[0] as usize + x;
                            live[i] = fresh[i];
                        }
                    }
                }
            }
            paint(&mut core, &cut, &mid, pre, [0, 0, 0, 255], pts);
            let after = shown(&core, &cut);
            assert!(after != before, "a borracha tirou algo");
            assert_same(&live, &after, w, &format!("raster, dureza {hardness}"));
        }
    }

    /// Vetor: trecho, até a interseção e traço inteiro — a prévia é o quadro com o comando do commit aplicado; também
    /// numa camada com movimento (escala e deslocamento quebrado: a prévia é redesenhada no canvas).
    #[test]
    fn live_vector_erase_equals_commit() {
        for (mode, moved) in [EraseMode::Segment, EraseMode::Intersection, EraseMode::Whole].into_iter().flat_map(|m| [(m, false), (m, true)]) {
            let (mut core, cut, mid) = doc(LayerType::Vector);
            if moved {
                cmds::set_layer_props(&mut core, &mid, serde_json::json!({"scale": 1.3, "offset": [5.5, -2.25]})).unwrap();
            }
            let cel = cmds::exposed_cel(&core.project, &cut, &mid, 0).unwrap();
            let mut cross = line(0.0, 0.0, 0.0, 4.0, [10, 10, 10]);
            cross.points = vec![StrokePoint { x: 30.0, y: 4.0, ..cross.points[0].clone() }, StrokePoint { x: 30.0, y: 60.0, ..cross.points[0].clone() }];
            let mut cross2 = cross.clone();
            cross2.points.iter_mut().for_each(|p| p.x = 66.0);
            // linha laranja com largura de ponto 1,6 (raio na tela maior que a meia largura nominal)
            let mut orange = line(20.0, 8.0, 88.0, 5.0, [250, 120, 0]);
            orange.points.iter_mut().for_each(|p| p.width = 1.6);
            let items = vec![
                IndexedStroke { index: 0, stroke: orange },
                IndexedStroke { index: 1, stroke: cross },
                IndexedStroke { index: 2, stroke: cross2 },
                IndexedStroke { index: 3, stroke: line(44.0, 8.0, 88.0, 2.0, [0, 120, 250]) },
            ];
            cmds::run(&mut core, Command::InsertStrokes { cel, items }).unwrap();
            let w = core.project.settings.width as usize;
            let before = shown(&core, &cut);
            let path: Vec<[f32; 2]> = (0..=10).map(|i| [48.0 + i as f32 * 0.3, 10.0 + i as f32 * 2.0]).collect();
            let sp = spec(&core, &cut, &mid, Mode::EraseVector, eraser(0.9, 8.0), mode, 4.0);
            let base = build_base(sp.project.as_ref().unwrap(), &sp.blobs, &BaseKey::of(sp.project.as_ref().unwrap(), &sp));
            let art = CelArt::new(sp.project.as_ref().unwrap(), &sp.blobs, sp.cel.as_ref().unwrap(), &mid);
            let req = Req { pts: Vec::new(), path: path.clone(), stroke: None };
            let o = vector_erase_overlay(&sp, &req, &base, &art).expect("a borracha toca a linha laranja");
            let live = with_overlay(&before, w, &o);
            let er = aiai_tools::erase::EraseVectorRequest { cut: cut.clone(), frame: 0, layer: mid.clone(), path, radius: 4.0, mode };
            let cmd = aiai_tools::erase::erase_vector_command(&core.project, &er).unwrap();
            cmds::run(&mut core, cmd).unwrap();
            let after = shown(&core, &cut);
            assert!(after != before, "{mode:?}: o commit apagou");
            assert_same(&live, &after, w, &format!("vetor {mode:?}, movida {moved}"));
            if moved {
                continue;
            }
            // nada da linha laranja sobra dentro do raio da borracha
            let orange = |c: Color32| c.r() > 200 && c.g() > 90 && c.g() < 150 && c.b() < 40;
            for y in 14..26usize {
                for x in 46..52usize {
                    assert!(!orange(after[y * w + x]), "{mode:?}: sobrou laranja em ({x}, {y})");
                }
            }
        }
    }

    /// Uma camada oculta (um fundo branco) não aparece onde a borracha tira: só o que o quadro mostra entra na base.
    #[test]
    fn hidden_layers_stay_out_of_the_erase_base() {
        let (mut core, cut, mid) = doc(LayerType::Raster);
        let white = add_layer(&mut core, &cut, LayerType::Raster, None).unwrap();
        let mut big = aiai_paint::brush::BrushPreset::round("papel", 200.0, 1.0);
        big.spacing = 0.05;
        paint(&mut core, &cut, &white, big, [255, 255, 255, 255], vec![vec![0.0, 32.0, 1.0], vec![96.0, 32.0, 1.0]]);
        cmds::set_layer_props(&mut core, &white, serde_json::json!({"visible": false})).unwrap();
        let sp = spec(&core, &cut, &mid, Mode::EraseRaster, eraser(1.0, 10.0), EraseMode::Segment, 5.0);
        let base = build_base(sp.project.as_ref().unwrap(), &sp.blobs, &BaseKey::of(sp.project.as_ref().unwrap(), &sp));
        // o papel oculto está acima da camada: fora da frente (longe da linha vetorial, nada)
        assert!(base.above.img.as_ref().is_some_and(|img| img.data[50 * 96 + 5][3] < 1e-3), "o papel oculto ficou fora da frente");
        // e embaixo da metade de baixo só o xadrez
        assert!(base.below.as_ref().unwrap().data[50 * 96 + 10][3] < 1e-3);
    }
}
