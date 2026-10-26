//! Miniaturas das camadas (painel Camadas, S25): o cel exposto de cada camada raster/vetor/preenchimento/referência
//! rasterizado em segundo plano (`aiai_render::render_cel_image`, o mesmo conteúdo cru que a mesclagem compõe) e reduzido
//! para a linha da lista. Cada miniatura guarda uma assinatura barata do cel (traços, tiles, preenchimentos, imagem); só
//! as que mudaram voltam para a fila, e nunca durante um gesto (o traço em andamento não espera por elas).

use super::commands::NodeId;
use super::render_service::Snapshot;
use aiai_core::CelData;
use crossbeam_channel::{Receiver, Sender};
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Maior lado da miniatura em pixels (a linha mostra em ~44×30 pontos; 2× para telas com escala).
pub(crate) const THUMB_SIDE: u32 = 96;

struct Job {
    layer: NodeId,
    cel: NodeId,
    sig: u64,
    canvas: (u32, u32),
    snap: Arc<Snapshot>,
}

struct Done {
    layer: NodeId,
    sig: u64,
    image: Option<ColorImage>,
}

#[derive(Default)]
pub(crate) struct LayerThumbs {
    /// Camada → (assinatura da miniatura pronta, textura).
    ready: HashMap<NodeId, (u64, TextureHandle)>,
    /// Pedidos em andamento (camada → assinatura pedida).
    asked: HashMap<NodeId, u64>,
    worker: Option<(Sender<Job>, Receiver<Done>)>,
}

/// Assinatura do conteúdo do cel (muda quando um traço, tile, preenchimento ou a imagem muda).
pub(crate) fn cel_sig(cel_id: &NodeId, c: &CelData, canvas: (u32, u32)) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cel_id.hash(&mut h);
    canvas.hash(&mut h);
    c.strokes.len().hash(&mut h);
    for s in &c.strokes {
        s.id.hash(&mut h);
        s.points.len().hash(&mut h);
        s.width.to_bits().hash(&mut h);
        format!("{:?}{:?}{:?}", s.points.first(), s.points.last(), s.color).hash(&mut h);
    }
    for t in &c.tiles {
        (t.x, t.y, &t.blob).hash(&mut h);
    }
    for f in &c.fills {
        (&f.id, &f.mask, format!("{:?}", f.color)).hash(&mut h);
    }
    c.image.hash(&mut h);
    c.text.hash(&mut h);
    h.finish()
}

/// Reduz a imagem do canvas (pré-multiplicada, sRGB) para caber em `side` × `side` (média por caixa).
pub(crate) fn downscale(img: &aiai_paint::Image, side: u32) -> ColorImage {
    let (w, h) = (img.width.max(1), img.height.max(1));
    let k = (side as f32 / w.max(h) as f32).min(1.0);
    let (tw, th) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
    let mut px = Vec::with_capacity((tw * th) as usize);
    for ty in 0..th {
        let (y0, y1) = (ty * h / th, ((ty + 1) * h / th).max(ty * h / th + 1));
        for tx in 0..tw {
            let (x0, x1) = (tx * w / tw, ((tx + 1) * w / tw).max(tx * w / tw + 1));
            let mut acc = [0.0f32; 4];
            // amostra no máximo 4×4 por célula (reduções grandes não precisam de todos os pixels)
            let sy = ((y1 - y0) / 4).max(1);
            let sx = ((x1 - x0) / 4).max(1);
            let mut n = 0.0;
            let mut y = y0;
            while y < y1 {
                let mut x = x0;
                while x < x1 {
                    let p = img.data[(y * w + x) as usize];
                    for i in 0..4 {
                        acc[i] += p[i];
                    }
                    n += 1.0;
                    x += sx;
                }
                y += sy;
            }
            let q = |v: f32| ((v / n).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            px.push(egui::Color32::from_rgba_premultiplied(q(acc[0]), q(acc[1]), q(acc[2]), q(acc[3])));
        }
    }
    ColorImage { size: [tw as usize, th as usize], source_size: egui::vec2(tw as f32, th as f32), pixels: px }
}

impl LayerThumbs {
    fn worker(&mut self, ctx: &egui::Context) -> Option<&(Sender<Job>, Receiver<Done>)> {
        if self.worker.is_none() {
            let (tx, jrx) = crossbeam_channel::unbounded::<Job>();
            let (dtx, rx) = crossbeam_channel::unbounded::<Done>();
            let c = ctx.clone();
            let spawned = std::thread::Builder::new().name("aiai-layer-thumbs".into()).spawn(move || {
                while let Ok(first) = jrx.recv() {
                    // pedidos acumulados: vale o mais novo de cada camada
                    let mut queue = vec![first];
                    queue.extend(jrx.try_iter());
                    let mut seen = HashSet::new();
                    for j in queue.into_iter().rev() {
                        if !seen.insert(j.layer.clone()) {
                            continue;
                        }
                        let image = match j.snap.project.node(&j.cel).map(|n| &n.data) {
                            Some(aiai_core::NodeKind::Cel(cd)) => {
                                let name = j.snap.project.node(&j.layer).map(|n| n.name.clone()).unwrap_or_default();
                                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| aiai_render::compose::render_cel_image(&j.snap.project, &j.snap.blobs, cd, false, j.canvas, &name)));
                                match r {
                                    Ok((img, _)) => Some(downscale(&img, THUMB_SIDE)),
                                    Err(_) => {
                                        tracing::warn!("miniatura da camada {}: o render falhou", j.layer.0);
                                        None
                                    }
                                }
                            }
                            _ => None,
                        };
                        if dtx.send(Done { layer: j.layer, sig: j.sig, image }).is_err() {
                            return;
                        }
                        c.request_repaint();
                    }
                }
            });
            match spawned {
                Ok(_) => self.worker = Some((tx, rx)),
                Err(e) => {
                    tracing::warn!("miniaturas das camadas: thread de fundo não abriu ({e})");
                    return None;
                }
            }
        }
        self.worker.as_ref()
    }

    /// Recolhe as miniaturas prontas (uma vez por quadro, antes de desenhar a lista).
    pub(crate) fn drain(&mut self, ctx: &egui::Context) {
        let Some((_, rx)) = &self.worker else { return };
        let done: Vec<Done> = rx.try_iter().collect();
        for d in done {
            if self.asked.get(&d.layer) == Some(&d.sig) {
                self.asked.remove(&d.layer);
            }
            match d.image {
                Some(img) => {
                    let tex = ctx.load_texture(format!("layer-thumb-{}", d.layer.0), img, TextureOptions::LINEAR);
                    self.ready.insert(d.layer, (d.sig, tex));
                }
                None => {
                    self.ready.remove(&d.layer);
                }
            }
        }
    }

    /// A miniatura da camada (a última pronta, mesmo se velha); pede uma nova quando o cel mudou e `idle`.
    pub(crate) fn get(&mut self, ctx: &egui::Context, layer: &NodeId, cel: &NodeId, sig: u64, canvas: (u32, u32), idle: bool, snap: impl FnOnce() -> Arc<Snapshot>) -> Option<egui::TextureId> {
        let have = self.ready.get(layer).map(|(s, t)| (*s, t.id()));
        let fresh = have.is_some_and(|(s, _)| s == sig);
        if !fresh && idle && self.asked.get(layer) != Some(&sig) {
            let job = Job { layer: layer.clone(), cel: cel.clone(), sig, canvas, snap: snap() };
            if let Some((tx, _)) = self.worker(ctx) {
                if tx.send(job).is_ok() {
                    self.asked.insert(layer.clone(), sig);
                }
            }
        }
        have.map(|(_, t)| t)
    }

    /// Esquece camadas que não existem mais.
    pub(crate) fn retain(&mut self, alive: &HashSet<NodeId>) {
        self.ready.retain(|k, _| alive.contains(k));
        self.asked.retain(|k, _| alive.contains(k));
    }

    /// Solta as texturas (orçamento de VRAM).
    pub(crate) fn release_gpu(&mut self) -> bool {
        let had = !self.ready.is_empty();
        self.ready.clear();
        had
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aiai_core::{BlobHash, TileEntry};

    #[test]
    fn signature_changes_with_the_content_only() {
        let id = aiai_core::Id("cel".into());
        let mut c = CelData::new(1);
        let a = cel_sig(&id, &c, (100, 100));
        assert_eq!(a, cel_sig(&id, &c.clone(), (100, 100)));
        c.tiles.push(TileEntry { x: 0, y: 0, blob: BlobHash::of(b"a") });
        let b = cel_sig(&id, &c, (100, 100));
        assert_ne!(a, b);
        c.tiles[0].blob = BlobHash::of(b"b");
        assert_ne!(b, cel_sig(&id, &c, (100, 100)));
        // o papel do cel não muda a miniatura
        let before = cel_sig(&id, &c, (100, 100));
        c.role = aiai_core::CelRole::Inbetween;
        assert_eq!(before, cel_sig(&id, &c, (100, 100)));
    }

    #[test]
    fn downscale_keeps_the_aspect_and_averages() {
        let mut img = aiai_paint::Image { width: 200, height: 100, data: vec![[0.0; 4]; 200 * 100] };
        for y in 0..100 {
            for x in 0..100 {
                img.data[y * 200 + x] = [1.0, 0.0, 0.0, 1.0];
            }
        }
        let t = downscale(&img, 96);
        assert_eq!(t.size, [96, 48]);
        let left = t.pixels[24 * 96 + 10];
        let right = t.pixels[24 * 96 + 80];
        assert_eq!(left.a(), 255);
        assert_eq!(right.a(), 0);
    }
}
