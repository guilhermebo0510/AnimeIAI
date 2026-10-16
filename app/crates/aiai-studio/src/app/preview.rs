//! Pré-visualização do documento no canvas: o primeiro corte do projeto, quadro 0, renderizado pelo motor CPU real
//! (`aiai_render`) numa thread de fundo e mostrado como textura dentro do quadro da câmera. É o mesmo render do
//! `RENDER` da bridge, então a captura da janela e o `aiai_render` mostram a mesma imagem. O canvas interativo (zoom,
//! pan, ferramentas) é da sprint do canvas; aqui o documento só precisa APARECER quando muda (pela UI ou pela bridge).

use aiai_core::{CoreSession, Id, NodeKind};
use std::time::Duration;

type Rendered = Option<(u32, u32, Vec<u8>)>;

pub(crate) struct CutPreview {
    /// `change_counter` da sessão no último render pedido (`None` = nunca).
    seen: Option<(u64, usize)>,
    job: Option<crossbeam_channel::Receiver<Rendered>>,
    texture: Option<egui::TextureHandle>,
}

impl Default for CutPreview {
    fn default() -> Self {
        CutPreview { seen: None, job: None, texture: None }
    }
}

/// Primeiro corte do primeiro episódio/cena (ordem do documento).
pub(crate) fn first_cut(core: &CoreSession) -> Option<Id> {
    let p = &core.project;
    for ep in &p.roots.episodes {
        for sc in &p.node(ep)?.children {
            for cut in &p.node(sc)?.children {
                if matches!(p.node(cut)?.data, NodeKind::Cut(_)) {
                    return Some(cut.clone());
                }
            }
        }
    }
    None
}

impl CutPreview {
    pub(crate) fn texture(&self) -> Option<&egui::TextureHandle> {
        self.texture.as_ref()
    }

    /// Chamado a cada quadro: recolhe o render pronto e, se o documento mudou, pede outro (um por vez).
    pub(crate) fn update(&mut self, ctx: &egui::Context, core: &CoreSession) {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok(res) => {
                    self.job = None;
                    match res {
                        Some((w, h, rgba)) => {
                            let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                            match &mut self.texture {
                                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                                None => self.texture = Some(ctx.load_texture("cut-preview", img, egui::TextureOptions::LINEAR)),
                            }
                        }
                        None => self.texture = None,
                    }
                    ctx.request_repaint();
                }
                Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(40)),
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.job = None,
            }
        }
        // identidade do documento: contador de mudanças + endereço da sessão não basta; o tamanho do log muda a cada passo
        let now = (core.change_counter, core.log().len());
        if self.job.is_some() || self.seen == Some(now) {
            return;
        }
        self.seen = Some(now);
        let Some(cut) = first_cut(core) else {
            self.texture = None;
            return;
        };
        let project = core.project.clone();
        let blobs = core.blobs.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new().name("aiai-cut-preview".into()).spawn(move || {
            let out = aiai_render::render_frame(&project, &blobs, &cut, 0, &aiai_render::RenderOptions::default()).ok().map(|o| (o.image.width, o.image.height, o.image.to_rgba8()));
            let _ = tx.send(out);
        });
        if spawned.is_ok() {
            self.job = Some(rx);
        }
    }
}
