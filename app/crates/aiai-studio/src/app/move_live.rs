//! Mover ao vivo: com a ferramenta Mover ativa, uma thread prepara a camada separada do resto do quadro (o que fica
//! abaixo, a parte que vai andar, a parte que fica, o que fica acima, com os efeitos de cor de cima aplicados em cada
//! uma). Durante o arrasto o canvas mostra essas quatro imagens no lugar do quadro, com só a parte que anda
//! transformada: é o conteúdo de verdade se movendo (raster ou vetor), sem re-render, e o commit sai ao soltar.

use super::commands::NodeId;
use aiai_core::*;
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use std::time::Instant;

/// O que determina as imagens: documento, corte, quadro, camada e a forma da seleção (sem o deslocamento).
#[derive(Clone, PartialEq, Debug)]
pub(crate) struct MoveKey {
    pub counter: u64,
    pub cut: NodeId,
    pub frame: u32,
    pub layer: NodeId,
    pub sel: String,
    pub guides: bool,
    pub reference: bool,
}

pub(crate) struct MoveTex {
    pub below: TextureHandle,
    pub stay: Option<TextureHandle>,
    pub moving: TextureHandle,
    pub above: TextureHandle,
    pub size: [usize; 2],
}

struct Images {
    below: ColorImage,
    stay: Option<ColorImage>,
    moving: ColorImage,
    above: ColorImage,
}

#[derive(Default)]
pub(crate) struct MoveLive {
    pub key: Option<MoveKey>,
    pending: Option<(MoveKey, crossbeam_channel::Receiver<Result<Images, String>>)>,
    pub tex: Option<MoveTex>,
    /// Depois de soltar: a transformação aplicada fica na tela até o render do documento novo chegar.
    pub hold: Option<(u64, aiai_paint::Affine, Instant)>,
    pub error: Option<String>,
}

pub(crate) struct Request {
    pub key: MoveKey,
    pub project: Project,
    pub blobs: BlobStore,
    /// Raster: a área (cobertura em px do canvas); `None` = a camada inteira.
    pub mask: Option<std::sync::Arc<aiai_paint::select::Selection>>,
    /// Vetor: os traços que andam (os mesmos que o commit leva: escolhidos ou os que a área pega, decididos no canvas
    /// em `selection::area_stroke_ids`); `None` = a camada inteira.
    pub strokes: Option<Vec<Id>>,
}

impl MoveLive {
    /// Pede as imagens para `key` (se já não estão prontas nem a caminho).
    pub(crate) fn want(&mut self, ctx: &egui::Context, make: impl FnOnce() -> Request, key: &MoveKey) {
        if self.key.as_ref() == Some(key) || self.pending.as_ref().is_some_and(|(k, _)| k == key) {
            return;
        }
        let req = make();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let ctx2 = ctx.clone();
        let spawned = std::thread::Builder::new().name("aiai-move".into()).spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&req))).unwrap_or_else(|_| Err("o preparo do mover entrou em pânico".into()));
            let _ = tx.send(r);
            ctx2.request_repaint();
        });
        if spawned.is_ok() {
            self.pending = Some((key.clone(), rx));
        }
    }

    pub(crate) fn poll(&mut self, ctx: &egui::Context, shown_counter: Option<u64>) {
        if let Some((key, rx)) = &self.pending {
            if let Ok(r) = rx.try_recv() {
                let key = key.clone();
                self.pending = None;
                match r {
                    Ok(im) => {
                        let opts = TextureOptions::LINEAR;
                        let size = im.below.size;
                        self.tex = Some(MoveTex {
                            below: ctx.load_texture("move-below", im.below, opts),
                            stay: im.stay.map(|s| ctx.load_texture("move-stay", s, opts)),
                            moving: ctx.load_texture("move-moving", im.moving, opts),
                            above: ctx.load_texture("move-above", im.above, opts),
                            size,
                        });
                        self.key = Some(key);
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        if let Some((c, _, t)) = self.hold {
            if shown_counter.is_some_and(|s| s >= c) || t.elapsed().as_secs_f32() > 5.0 {
                self.hold = None;
            }
        }
    }

    /// As imagens desta chave ainda não estão prontas nem a caminho?
    pub(crate) fn wants(&self, key: &MoveKey) -> bool {
        self.key.as_ref() != Some(key) && !self.pending.as_ref().is_some_and(|(k, _)| k == key)
    }

    /// Imagens prontas para esta chave?
    pub(crate) fn ready_for(&self, key: &MoveKey) -> bool {
        self.tex.is_some() && self.key.as_ref() == Some(key)
    }
}

fn to_color_image(img: &aiai_paint::Image) -> ColorImage {
    let px = |p: &[f32; 4]| {
        let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Color32::from_rgba_premultiplied(b(p[0]), b(p[1]), b(p[2]), b(p[3]))
    };
    ColorImage { size: [img.width as usize, img.height as usize], source_size: egui::vec2(img.width as f32, img.height as f32), pixels: img.data.iter().map(px).collect() }
}

fn build(req: &Request) -> Result<Images, String> {
    let p = &req.project;
    let k = &req.key;
    let mut all = Vec::new();
    super::live::ordered_leaves(p, &k.cut, &mut all);
    let pos = all.iter().position(|(id, _)| *id == k.layer).ok_or("a camada não está no corte")?;
    let is_fx = |l: &LayerData| matches!(l.kind, LayerType::Effect | LayerType::Adjustment);
    let fx_above: Vec<NodeId> = all[pos + 1..].iter().filter(|(_, l)| l.visible && is_fx(l)).map(|(id, _)| id.clone()).collect();
    let mut below: Vec<NodeId> = all[..pos].iter().filter(|(_, l)| l.visible).map(|(id, _)| id.clone()).collect();
    below.extend(fx_above.iter().cloned());
    let mut above: Vec<NodeId> = all[pos + 1..].iter().filter(|(_, l)| l.visible && !is_fx(l)).map(|(id, _)| id.clone()).collect();
    above.extend(fx_above.iter().cloned());
    let mut solo = vec![k.layer.clone()];
    solo.extend(fx_above.iter().cloned());
    let render = |proj: &Project, ids: Vec<NodeId>| -> Result<aiai_paint::Image, String> {
        let opts = aiai_render::RenderOptions { include_guides: k.guides, include_reference: k.reference, only_layers: Some(ids), background: None, camera: false, layer_motion: true };
        aiai_render::render_frame(proj, &req.blobs, &k.cut, k.frame, &opts).map(|o| o.image).map_err(|e| e.to_string())
    };
    let below_img = render(p, below)?;
    let above_img = render(p, above)?;
    let kind = all[pos].1.kind;
    let (moving, stay) = match (&req.strokes, &req.mask) {
        (Some(ids), _) if kind == LayerType::Vector => {
            let cut = match p.node(&k.cut).map(|n| &n.data) {
                Some(NodeKind::Cut(c)) => c,
                _ => return Err("corte".into()),
            };
            let cel_id = aiai_tools::context::exposed_cel(p, cut, &k.layer, k.frame).map(|n| n.id.clone()).ok_or("sem cel no quadro")?;
            let split = |want: bool| -> Result<aiai_paint::Image, String> {
                let mut q = p.clone();
                if let Some(NodeKind::Cel(c)) = q.node_mut(&cel_id).map(|n| &mut n.data) {
                    c.strokes.retain(|s| ids.contains(&s.id) == want);
                }
                render(&q, solo.clone())
            };
            (split(true)?, Some(split(false)?))
        }
        (_, Some(sel)) if kind != LayerType::Vector => {
            let full = render(p, solo.clone())?;
            let (mut m, mut s) = (full.clone(), full);
            for (i, &c) in sel.cov.iter().enumerate().take(m.data.len()) {
                for ch in 0..4 {
                    m.data[i][ch] *= c;
                    s.data[i][ch] *= 1.0 - c;
                }
            }
            (m, Some(s))
        }
        _ => (render(p, solo)?, None),
    };
    Ok(Images { below: to_color_image(&below_img), stay: stay.as_ref().map(to_color_image), moving: to_color_image(&moving), above: to_color_image(&above_img) })
}
