//! Prévia ao vivo do pincel e da borracha: enquanto o traço é arrastado, uma thread roda o MESMO caminho do commit
//! (`paint_on_cel` do pincel/borracha raster, `erase_vector_command` da borracha vetorial) num recorte da camada, e o
//! canvas mostra o resultado por cima do quadro. Antes a prévia era uma polilinha (pincel) ou uma faixa branca
//! (borracha) e o traço "mudava" ao soltar.
//!
//! - Caneta: o traço vetorial rasterizado pelo mesmo motor do render (largura, pontas, antisserrilhado), com as
//!   camadas de cima na frente, em vez do polígono desenhado pela interface.
//! - Pincel: o recorte pintado, só nos pixels que mudaram, com a opacidade da camada, com as camadas de cima na frente
//!   (renderizadas uma vez no começo do traço) e os efeitos de cor de cima (Grade) aplicados.
//! - Borracha: o que fica embaixo da camada (o quadro renderizado sem ela, sobre o xadrez do canvas) na proporção do
//!   que a borracha tira de cada pixel: o resultado exato para camada em modo normal.
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
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let (tx, rx) = crossbeam_channel::bounded(2);
        let (sh, ctx2) = (Arc::clone(&shared), ctx.clone());
        let spawned = std::thread::Builder::new().name("aiai-live".into()).spawn(move || worker(spec, sh, tx, ctx2));
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
                        self.overlay = Some(Overlay { origin: o.origin, size, tex, fwd: self.fwd });
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
}

fn worker(spec: StartSpec, sh: Arc<(Mutex<State>, Condvar)>, tx: crossbeam_channel::Sender<Out>, ctx: egui::Context) {
    // borracha: o quadro sem a camada, sobre o xadrez (o que aparece onde ela tira); pincel: o que fica na frente
    let under = match spec.mode {
        Mode::Brush => None,
        _ => spec.project.as_ref().and_then(|p| render_under(p, &spec)),
    };
    let above = match spec.mode {
        Mode::Brush | Mode::Pen => spec.project.as_ref().map(|p| render_above(p, &spec)),
        _ => None,
    };
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
            Mode::EraseRaster => under.as_ref().and_then(|u| raster_erase_overlay(&spec, &req, u, inc.as_mut()?)),
            Mode::EraseVector => under.as_ref().and_then(|u| vector_erase_overlay(&spec, &req, u)),
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

/// Camadas desenháveis do corte (pastas abertas), menos `skip`.
fn leaves_except(p: &Project, parent: &NodeId, skip: &NodeId, out: &mut Vec<NodeId>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        if c == skip {
            continue;
        }
        match p.node(c).map(|x| &x.data) {
            Some(NodeKind::Layer(l)) if l.kind == LayerType::Group => leaves_except(p, c, skip, out),
            Some(NodeKind::Layer(_)) => out.push(c.clone()),
            _ => {}
        }
    }
}

/// Camadas desenháveis do corte de baixo para cima (pastas abertas), com os dados.
fn ordered_leaves(p: &Project, parent: &NodeId, out: &mut Vec<(NodeId, LayerData)>) {
    let Some(n) = p.node(parent) else { return };
    for c in &n.children {
        match p.node(c).map(|x| &x.data) {
            Some(NodeKind::Layer(l)) if l.kind == LayerType::Group => ordered_leaves(p, c, out),
            Some(NodeKind::Layer(l)) => out.push((c.clone(), l.clone())),
            _ => {}
        }
    }
}

fn render_above(p: &Project, spec: &StartSpec) -> Above {
    let mut all = Vec::new();
    ordered_leaves(p, &spec.cut, &mut all);
    let pos = all.iter().position(|(id, _)| *id == spec.layer).map(|i| i + 1).unwrap_or(all.len());
    let ids: Vec<NodeId> = all[pos..].iter().filter(|(_, l)| l.visible && !matches!(l.kind, LayerType::Effect | LayerType::Adjustment)).map(|(id, _)| id.clone()).collect();
    let fx = all[pos..]
        .iter()
        .filter(|(_, l)| l.visible && matches!(l.kind, LayerType::Effect | LayerType::Adjustment))
        .filter_map(|(_, l)| aiai_fx::stack_from_params(&l.params).ok().map(|st| (st, l.keys.eval("opacity", spec.frame as f32).unwrap_or(l.opacity))))
        .collect();
    let img = (!ids.is_empty())
        .then(|| {
            let opts = aiai_render::RenderOptions { include_guides: spec.guides, include_reference: spec.reference, only_layers: Some(ids), background: None, camera: false, layer_motion: true };
            aiai_render::render_frame(p, &spec.blobs, &spec.cut, spec.frame, &opts).ok().map(|o| o.image)
        })
        .flatten();
    Above { img, fx }
}

/// Quadro sem a camada, composto sobre o xadrez do canvas: RGB opaco em px do canvas.
fn render_under(p: &Project, spec: &StartSpec) -> Option<(u32, u32, Vec<[f32; 3]>)> {
    let mut others = Vec::new();
    leaves_except(p, &spec.cut, &spec.layer, &mut others);
    let opts = aiai_render::RenderOptions { include_guides: spec.guides, include_reference: spec.reference, only_layers: Some(others), background: None, camera: false, layer_motion: true };
    let img = aiai_render::render_frame(p, &spec.blobs, &spec.cut, spec.frame, &opts).ok()?.image;
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
    Some((img.width, img.height, rgb))
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
            [x, y, pr] => Some(BrushPoint { x: *x, y: *y, pressure: pr.clamp(0.0, 1.0) }),
            _ => None,
        }).collect();
        let n = pts.len();
        if n == 0 {
            return None;
        }
        let smudge = (spec.preset.mix > 0.0).then_some(&self.layer);
        // o último ponto ainda pode mudar (espaçamento das amostras): pronto é o trecho que não depende dele
        while n >= 4 && self.fed + 4 <= n {
            let seg = aiai_tools::paint::spline_segment(&pts, self.fed);
            self.sb.add(smudge, &seg);
            self.fed += 1;
        }
        let committed = self.sb.take_dirty();
        let mut tmp = self.sb.clone();
        if n < 3 {
            tmp.add(smudge, &pts);
        } else {
            for i in self.fed..n - 1 {
                tmp.add(smudge, &aiai_tools::paint::spline_segment(&pts, i));
            }
            tmp.add(smudge, &pts[n - 1..]);
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
    let r = inc.region(spec, req)?;
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
fn dress(spec: &StartSpec, origin: [i32; 2], mut crop: aiai_paint::Image, mask: &[bool], above: Option<&Above>) -> Out {
    let (cw, ch) = (crop.width as usize, crop.height as usize);
    // as camadas de cima ficam na frente do traço (a cerca na frente da casa)
    if let Some(img) = above.and_then(|ab| ab.img.as_ref()) {
        for y in 0..ch {
            for x in 0..cw {
                let i = y * cw + x;
                if !mask[i] {
                    continue;
                }
                let (px0, py0) = ((origin[0] + x as i32) as f32 + 0.5, (origin[1] + y as i32) as f32 + 0.5);
                let (cx, cy) = match &spec.fwd {
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
    Out { origin, image: ColorImage { size: [cw, ch], source_size: egui::vec2(cw as f32, ch as f32), pixels }, partial: None, ms: 0.0 }
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

fn erase_pixels(x0: i32, y0: i32, w: usize, h: usize, amount: impl Fn(usize, usize) -> f32, u: &(u32, u32, Vec<[f32; 3]>), fwd: &Option<Affine>) -> Out {
    let mut pixels = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let k = amount(x, y).clamp(0.0, 1.0);
            let c = if k > 0.0 { under_at(u, fwd, (x0 + x as i32) as f32 + 0.5, (y0 + y as i32) as f32 + 0.5) } else { None };
            pixels.push(match c {
                Some(c) => px([c[0] * k, c[1] * k, c[2] * k, k]),
                None => Color32::TRANSPARENT,
            });
        }
    }
    Out { origin: [x0, y0], image: ColorImage { size: [w, h], source_size: egui::vec2(w as f32, h as f32), pixels }, partial: None, ms: 0.0 }
}

fn raster_erase_overlay(spec: &StartSpec, req: &Req, u: &(u32, u32, Vec<[f32; 3]>), inc: &mut Incremental) -> Option<Out> {
    let r = inc.region(spec, req)?;
    let before = r.before.as_ref()?;
    let (x0, y0, x1, y1) = crop_of(&r);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let w = r.after.width as usize;
    let k = spec.layer_opacity.clamp(0.0, 1.0);
    // quanto da camada saiu do pixel × quanto a camada pesava nele (alfa × opacidade): o quadro vai até o "embaixo"
    let amount = |x: usize, y: usize| {
        let (a, b) = (r.after.data[(y0 + y) * w + x0 + x][3], before.data[(y0 + y) * w + x0 + x][3]);
        (b - a).max(0.0) * k
    };
    let mut o = erase_pixels(r.origin[0] + x0 as i32, r.origin[1] + y0 as i32, x1 - x0, y1 - y0, amount, u, &spec.fwd);
    o.partial = Some([spec.canvas.0 as usize, spec.canvas.1 as usize]);
    Some(o)
}

fn vector_erase_overlay(spec: &StartSpec, req: &Req, u: &(u32, u32, Vec<[f32; 3]>)) -> Option<Out> {
    let p = spec.project.as_ref()?;
    if req.path.is_empty() {
        return None;
    }
    let er = aiai_tools::erase::EraseVectorRequest { cut: spec.cut.clone(), frame: spec.frame, layer: spec.layer.clone(), path: req.path.clone(), radius: spec.erase_radius, mode: spec.erase_mode };
    let Command::Batch { commands, .. } = aiai_tools::erase::erase_vector_command(p, &er).ok()? else { return None };
    let cel = spec.cel.as_ref()?;
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    for c in &commands {
        match c {
            Command::RemoveStrokes { ids, .. } => removed.extend(cel.strokes.iter().filter(|s| ids.contains(&s.id))),
            Command::InsertStrokes { items, .. } => kept.extend(items.iter().map(|i| &i.stroke)),
            _ => {}
        }
    }
    if removed.is_empty() {
        return None;
    }
    // cobertura (no espaço do cel) do que sai menos a dos pedaços que ficam
    let (w, h) = spec.canvas;
    let cover = |strokes: &[&Stroke], acc: &mut Vec<f32>, sign: f32| {
        for s in strokes {
            let smp = aiai_paint::sample_stroke(s);
            if let Some(c) = aiai_paint::stroke_raster::rasterize_stroke(&smp, s.aliased, w, h) {
                for yy in 0..c.height {
                    for xx in 0..c.width {
                        let (x, y) = (c.x0 + xx as i32, c.y0 + yy as i32);
                        if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                            let i = (y as u32 * w + x as u32) as usize;
                            acc[i] = (acc[i] + sign * c.cov[(yy * c.width + xx) as usize]).clamp(0.0, 1.0);
                        }
                    }
                }
            }
        }
    };
    let mut m = vec![0.0f32; (w * h) as usize];
    cover(&removed, &mut m, 1.0);
    let kept: Vec<&Stroke> = kept;
    cover(&kept, &mut m, -1.0);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0u32, 0u32);
    for (i, v) in m.iter().enumerate() {
        if *v > 0.0 {
            let (x, y) = (i as u32 % w, i as u32 / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x + 1);
            y1 = y1.max(y + 1);
        }
    }
    if x0 >= x1 {
        return None;
    }
    let k = spec.layer_opacity.clamp(0.0, 1.0);
    let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    let amount = |x: usize, y: usize| m[((y0 as usize + y) * w as usize) + x0 as usize + x] * k;
    Some(erase_pixels(x0 as i32, y0 as i32, bw, bh, amount, u, &spec.fwd))
}
