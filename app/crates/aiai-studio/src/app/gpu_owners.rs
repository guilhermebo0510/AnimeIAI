//! Quem guarda texturas do egui e como solta cada uma (ver `gpu_evict`): por dono que sumiu da tela e, sob pressão de
//! VRAM, os caches de miniaturas. Toda textura aqui é refeita pelo dono quando volta a aparecer (miniatura, prévia,
//! sobreposição), então soltar nunca perde trabalho: custa no máximo um render de novo.
//!
//! Medida (projeto ultima-rosa, 1600×900, todos os workspaces visitados): sem isto as texturas do egui só cresciam
//! (1 → 53 MB): três prévias de 1920×1080 (Edição, monitor do Áudio, ficha), as quatro imagens do Mover ao vivo do
//! tamanho do canvas que ficavam depois de trocar de ferramenta, e o canvas fora dos workspaces que o mostram.

use super::StudioApp;
use crate::gpu_mem::Pressure;
use crate::layout::PanelKind;
use crate::workspace::{ToolId, WorkspaceId};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Donos de textura acompanhados (cada um sabe se está na tela e como soltar).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    /// Imagem do canvas, mesa de luz e prévia do arrasto 3D: painel Canvas.
    Canvas,
    /// As quatro imagens do Mover ao vivo: só com a ferramenta Mover num canvas na tela.
    MoveLive,
    /// Imagens do arrasto da camada 3D: só com a ferramenta 3D.
    Drag3d,
    EditPreview,
    EditSource,
    AudioMonitor,
    CharSheet,
    CharRef,
    Modeling,
    LibraryPreview,
    LibraryGrid,
    SubView,
    Spectrogram,
    FontPreviews,
    Poster,
    DeliverThumbs,
    BoardThumbs,
    CutThumbs,
    EffectThumbs,
    CompAlpha,
    /// Miniaturas das camadas (painel Camadas, S25).
    LayerThumbs,
}

const SLOTS: [Slot; 21] = [
    Slot::Canvas,
    Slot::MoveLive,
    Slot::Drag3d,
    Slot::EditPreview,
    Slot::EditSource,
    Slot::AudioMonitor,
    Slot::CharSheet,
    Slot::CharRef,
    Slot::Modeling,
    Slot::LibraryPreview,
    Slot::LibraryGrid,
    Slot::SubView,
    Slot::Spectrogram,
    Slot::FontPreviews,
    Slot::Poster,
    Slot::DeliverThumbs,
    Slot::BoardThumbs,
    Slot::CutThumbs,
    Slot::EffectThumbs,
    Slot::CompAlpha,
    Slot::LayerThumbs,
];

impl Slot {
    /// Caches de miniaturas: com pressão normal ficam mesmo fora da tela (refazer custa renders na CPU); sob pressão
    /// saem como as prévias, e na crítica saem mesmo na tela (voltam aos poucos pelo limite de renders por quadro).
    fn is_cache(self) -> bool {
        matches!(self, Slot::LibraryGrid | Slot::FontPreviews | Slot::DeliverThumbs | Slot::BoardThumbs | Slot::CutThumbs | Slot::EffectThumbs | Slot::LayerThumbs)
    }
}

/// Último quadro em que cada dono apareceu.
#[derive(Default)]
pub(crate) struct Seen {
    at: HashMap<Slot, Instant>,
    /// Última limpeza dos caches na tela sob pressão crítica (uma a cada [`CRITICAL_FLUSH`], sem refazer e soltar a
    /// cada quadro).
    flushed: Option<Instant>,
}

const CRITICAL_FLUSH: Duration = Duration::from_secs(10);

fn drawn(app: &StudioApp, k: PanelKind) -> bool {
    app.rects.get(&format!("panel:{}", k.key())).is_some()
}

fn visible(app: &StudioApp, s: Slot) -> bool {
    let ws = app.ws;
    match s {
        Slot::Canvas => drawn(app, PanelKind::Canvas),
        Slot::MoveLive => drawn(app, PanelKind::Canvas) && app.tool == ToolId::Move,
        Slot::Drag3d => drawn(app, PanelKind::Canvas) && app.tool == ToolId::Model3d,
        Slot::EditPreview => ws == WorkspaceId::Edicao && drawn(app, PanelKind::Preview),
        Slot::EditSource => drawn(app, PanelKind::SourceMonitor),
        Slot::AudioMonitor => ws == WorkspaceId::Audio && drawn(app, PanelKind::Preview),
        Slot::CharSheet => drawn(app, PanelKind::Sheet),
        Slot::CharRef => drawn(app, PanelKind::CharRef),
        Slot::Modeling => drawn(app, PanelKind::Modeling),
        Slot::LibraryPreview => drawn(app, PanelKind::LibraryPreview),
        Slot::LibraryGrid => drawn(app, PanelKind::Library),
        Slot::SubView => drawn(app, PanelKind::SubView),
        Slot::LayerThumbs => drawn(app, PanelKind::Layers),
        Slot::Spectrogram => drawn(app, PanelKind::Waveform),
        Slot::FontPreviews => drawn(app, PanelKind::Text),
        Slot::Poster => drawn(app, PanelKind::Poster),
        Slot::DeliverThumbs => drawn(app, PanelKind::Deliverables) || drawn(app, PanelKind::Queue),
        Slot::BoardThumbs => drawn(app, PanelKind::Board),
        Slot::CutThumbs => drawn(app, PanelKind::Episodes) || drawn(app, PanelKind::Tasks) || drawn(app, PanelKind::ProdCharts) || ws == WorkspaceId::Producao,
        Slot::EffectThumbs => drawn(app, PanelKind::EffectsBrowser),
        Slot::CompAlpha => ws == WorkspaceId::Composicao && drawn(app, PanelKind::Canvas),
    }
}

/// Solta o que o dono `s` guarda. Devolve se havia algo.
fn release(app: &mut StudioApp, s: Slot) -> bool {
    let ed = &mut app.ed;
    match s {
        Slot::Canvas => {
            let mut had = ed.shown.take().is_some();
            had |= ed.anim.release_onion();
            had |= ed.comp.alpha_tex.take().is_some();
            had
        }
        Slot::MoveLive => {
            // nunca no meio de um arrasto nem enquanto o resultado solto ainda está na tela
            if !matches!(ed.gesture, super::editor::Gesture::None) || ed.move_live.hold.is_some() {
                return false;
            }
            ed.move_live.key = None;
            ed.move_live.tex.take().is_some()
        }
        Slot::Drag3d => ed.m3d.release_gpu(),
        Slot::EditPreview => ed.edit.preview.release_gpu(),
        Slot::EditSource => ed.edit.source.preview.release_gpu(),
        Slot::AudioMonitor => ed.audio.monitor.release_gpu(),
        Slot::CharSheet => ed.chars.sheet.release_gpu(),
        Slot::CharRef => ed.anim.char_ref.preview.release_gpu(),
        Slot::Modeling => ed.modeling.release_gpu(),
        Slot::LibraryPreview => ed.library.release_gpu(false),
        Slot::LibraryGrid => ed.library.release_gpu(true),
        Slot::SubView => ed.subview.release_gpu(),
        Slot::Spectrogram => ed.audio_ws.wave.release_gpu(),
        Slot::FontPreviews => ed.text.panel.release_gpu(),
        Slot::Poster => {
            let p = &mut ed.deliver.poster;
            p.asked = None;
            p.shown = None;
            p.tex.take().is_some()
        }
        Slot::DeliverThumbs => {
            let had = !ed.deliver.thumbs.is_empty();
            ed.deliver.thumbs.clear();
            had
        }
        Slot::BoardThumbs => ed.thumbs.clear(),
        Slot::CutThumbs => ed.prod.thumbs.clear() | ed.prod.compare.take().is_some(),
        Slot::EffectThumbs => {
            let had = !ed.comp.thumbs.is_empty();
            ed.comp.thumbs.clear();
            had
        }
        Slot::CompAlpha => ed.comp.alpha_tex.take().is_some(),
        Slot::LayerThumbs => ed.workbar.thumbs.release_gpu(),
    }
}

/// Atualiza quem está na tela e solta o que sumiu: prévias e sobreposições depois de
/// [`super::gpu_evict::HIDDEN_RELEASE`] (na hora, sob pressão); caches só sob pressão; na pressão crítica, os caches
/// mesmo na tela. Devolve se algo foi solto.
pub(crate) fn release_hidden(app: &mut StudioApp, p: Pressure) -> bool {
    let now = Instant::now();
    let mut acted = false;
    let flush = p == Pressure::Critical && app.gpu_seen.flushed.is_none_or(|t| now.duration_since(t) >= CRITICAL_FLUSH);
    if flush {
        app.gpu_seen.flushed = Some(now);
    }
    for s in SLOTS {
        let on = visible(app, s);
        if on {
            app.gpu_seen.at.insert(s, now);
        }
        let hidden_for = if on { Duration::ZERO } else { app.gpu_seen.at.get(&s).map_or(Duration::MAX, |t| now.duration_since(*t)) };
        let go = match (s.is_cache(), p) {
            (true, Pressure::Critical) => flush || !on,
            (true, Pressure::Normal) => false,
            (true, _) => !on,
            (false, Pressure::Normal) => !on && hidden_for >= super::gpu_evict::HIDDEN_RELEASE,
            (false, _) => !on,
        };
        if go {
            acted |= release(app, s);
        }
    }
    acted
}
