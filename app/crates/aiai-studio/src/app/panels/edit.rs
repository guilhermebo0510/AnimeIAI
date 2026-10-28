//! Workspace Edição (S24 3.13–3.18, estilo Premiere): painel Mídia (documentos de edição, cortes, sons e imagens do
//! projeto; duplo clique abre no monitor de origem, arrastar solta na linha do tempo), monitor do programa (o quadro da
//! edição como o MP4 vai mostrar, com reprodução e som), Propriedades (item, transição ou a edição inteira) e o estado
//! comum dos painéis da edição. A linha do tempo mora em `edit_timeline`; o monitor de origem, as Transições, os
//! Efeitos do clipe e as Legendas em `edit_panels`.
//!
//! Toda mudança regrava o documento inteiro por `EDIT_SET` (a mesma ação do MCP; um passo de desfazer por gesto: o
//! arraste na linha do tempo mexe numa cópia e só grava ao soltar). O motor valida antes de gravar (sobreposição,
//! fonte inexistente): o erro volta como aviso e o documento não muda. Documento v1 vira v2 ao abrir; só é regravado
//! quando o usuário mexer.

use super::commit_on_release;
use super::production::{CutThumbs, THUMB_BUDGET};
use crate::app::actions::{self, DocKind};
use crate::app::commands::NodeId;
use crate::app::ui_kit::{Env, body_ui, dim_label, section};
use crate::i18n::tr;
use aiai_core::{AssetType, CoreSession, Id, NodeKind, Project};
use aiai_tools::edit::captions::CaptionLimits;
use aiai_tools::edit::{EditDoc, EditItem, ItemSource, Timeline, TrackKind, Transition, TransitionAlign, TransitionKind, load_edit, media_frames};
use egui::{Color32, FontId, Id as EId, Rect, Sense, Stroke, StrokeKind, Ui};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Transições na ordem do painel, com a chave i18n do nome.
pub(crate) fn transition_key(k: TransitionKind) -> String {
    format!("editing.tr.{}", k.key())
}

/// Documento de edição aberto + linha do tempo (o erro de validação/leitura fica guardado como texto).
pub(crate) struct Opened {
    pub id: NodeId,
    pub doc: Arc<EditDoc>,
    pub tl: Result<Timeline, String>,
}

/// Prévia de um quadro em segundo plano (um render por vez; o pedido mais novo vence).
#[derive(Default)]
pub(crate) struct EditPreview {
    /// (change_counter, chave, quadro) do último render pedido.
    asked: Option<(u64, String, u32)>,
    job: Option<crossbeam_channel::Receiver<(u32, Result<(u32, u32, Vec<u8>), String>)>>,
    pub texture: Option<egui::TextureHandle>,
    pub shown_frame: Option<u32>,
    pub error: Option<String>,
}

impl EditPreview {
    /// Solta a textura (orçamento de VRAM, `gpu_owners`): volta pedida de novo quando o painel aparecer.
    pub(crate) fn release_gpu(&mut self) -> bool {
        self.asked = None;
        self.shown_frame = None;
        self.texture.take().is_some()
    }

    pub(crate) fn update(&mut self, ctx: &egui::Context, core: &CoreSession, key: String, doc: &Arc<EditDoc>, frame: u32) {
        self.request(ctx, (core.change_counter, key, frame), || {
            let (project, blobs, doc) = (core.project.clone(), core.blobs.clone(), doc.clone());
            move || aiai_tools::edit::render_edit_frame(&project, &blobs, &doc, frame, true).map_err(|e| e.to_string())
        });
    }

    /// Recolhe o render pronto e, se `key` (change_counter, documento, quadro) mudou, pede outro com `render` (um por vez;
    /// o pedido mais novo vence). `make` (que copia o projeto para a thread) só roda quando há render novo a fazer.
    pub(crate) fn request<F>(&mut self, ctx: &egui::Context, key: (u64, String, u32), make: impl FnOnce() -> F)
    where
        F: FnOnce() -> Result<aiai_paint::Image, String> + Send + 'static,
    {
        if let Some(rx) = &self.job {
            match rx.try_recv() {
                Ok((f, res)) => {
                    self.job = None;
                    match res {
                        Ok((w, h, rgba)) => {
                            // sob pressão de VRAM a prévia vem menor (o monitor escala para o retângulo dele)
                            let img = crate::gpu_mem::scale_for_pressure(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba));
                            match &mut self.texture {
                                Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                                None => self.texture = Some(ctx.load_texture("edit-preview", img, egui::TextureOptions::LINEAR)),
                            }
                            self.shown_frame = Some(f);
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                    ctx.request_repaint();
                }
                Err(crossbeam_channel::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(30)),
                Err(crossbeam_channel::TryRecvError::Disconnected) => self.job = None,
            }
        }
        if self.job.is_some() || self.asked.as_ref() == Some(&key) {
            return;
        }
        let frame = key.2;
        self.asked = Some(key);
        let render = make();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let spawned = std::thread::Builder::new().name("aiai-preview".into()).spawn(move || {
            let out = render().map(|img| (img.width, img.height, img.to_rgba8()));
            let _ = tx.send((frame, out));
        });
        if spawned.is_ok() {
            self.job = Some(rx);
        }
    }
}

/// Reprodução com som: o relógio só começa quando o som está pronto (mixagem em segundo plano); o monitor mostra o
/// quadro que o render de prévia devolve (rascunho: pula quadros se o render não acompanhar).
pub(crate) struct Play {
    pub start_frame: u32,
    pub started: Option<Instant>,
    pub mix: Option<crossbeam_channel::Receiver<Result<Vec<f32>, String>>>,
    /// WAV tocando (o winmm lê da memória até parar).
    pub wav: Option<Vec<u8>>,
    /// Velocidade (J/K/L: 1, 2, 4; negativa = para trás, sem som).
    pub rate: f64,
}

impl Play {
    /// Começa em `start` com o som de `mix` (em segundo plano); sem som ao vivo, o relógio corre já.
    pub(crate) fn start<F>(start: u32, rate: f64, with_sound: bool, mix: F) -> Play
    where
        F: FnOnce() -> Result<Vec<f32>, String> + Send + 'static,
    {
        let rx = (with_sound && rate == 1.0).then(|| {
            let (tx, rx) = crossbeam_channel::bounded(1);
            let _ = std::thread::Builder::new().name("aiai-edit-mix".into()).spawn(move || {
                let _ = tx.send(mix());
            });
            rx
        });
        let started = if rx.is_none() { Some(Instant::now()) } else { None };
        Play { start_frame: start, started, mix: rx, wav: None, rate }
    }

    /// Avança o relógio; devolve o quadro atual (ou `None` quando passou do fim, e a reprodução deve parar).
    pub(crate) fn tick(&mut self, fps: f64, sample_rate: u32, last: u32) -> Option<u32> {
        if let Some(rx) = &self.mix {
            match rx.try_recv() {
                Ok(res) => {
                    self.mix = None;
                    if let Ok(stereo) = res {
                        let sr = sample_rate.max(8000);
                        let from = (self.start_frame as f64 / fps * sr as f64) as usize;
                        let wav = crate::app::audio_out::wav_from_interleaved(&stereo, sr, from);
                        crate::app::audio_out::play_wav(Some(&wav));
                        self.wav = Some(wav);
                    }
                    self.started = Some(Instant::now());
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    self.mix = None;
                    self.started = Some(Instant::now());
                }
            }
        }
        let Some(t0) = self.started else { return Some(self.start_frame) };
        let f = self.start_frame as f64 + t0.elapsed().as_secs_f64() * fps * self.rate;
        if f < 0.0 || f > last as f64 {
            return None;
        }
        Some(f as u32)
    }

    pub(crate) fn stop(self) {
        if self.wav.is_some() {
            crate::app::audio_out::play_wav(None);
        }
    }
}

/// Ferramenta da linha do tempo (atalhos do Premiere, só na Edição).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum EditTool {
    #[default]
    Select,
    Razor,
    Slip,
    Slide,
    Ripple,
    Roll,
    Hand,
    Zoom,
}

impl EditTool {
    pub const ALL: [EditTool; 8] = [EditTool::Select, EditTool::Ripple, EditTool::Roll, EditTool::Razor, EditTool::Slip, EditTool::Slide, EditTool::Hand, EditTool::Zoom];

    pub fn key(self) -> &'static str {
        match self {
            EditTool::Select => "select",
            EditTool::Razor => "razor",
            EditTool::Slip => "slip",
            EditTool::Slide => "slide",
            EditTool::Ripple => "ripple",
            EditTool::Roll => "roll",
            EditTool::Hand => "hand",
            EditTool::Zoom => "zoom",
        }
    }

    /// Tecla do Premiere.
    pub fn shortcut(self) -> egui::Key {
        use egui::Key as K;
        match self {
            EditTool::Select => K::V,
            EditTool::Razor => K::C,
            EditTool::Slip => K::Y,
            EditTool::Slide => K::U,
            EditTool::Ripple => K::B,
            EditTool::Roll => K::N,
            EditTool::Hand => K::H,
            EditTool::Zoom => K::Z,
        }
    }
}

/// Qual monitor recebe I/O, espaço e J/K/L.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Focus {
    Source,
    #[default]
    Program,
}

/// Picos por quadro da edição (fração de um quadro do projeto por pico).
pub(crate) const WAVE_PER_FRAME: usize = 4;

enum WaveSlot {
    Pending(crossbeam_channel::Receiver<Vec<f32>>),
    Ready(Arc<Vec<f32>>),
}

/// Formas de onda das fontes de som na linha do tempo da Edição: `WAVE_PER_FRAME` picos por quadro do projeto, feitas
/// numa thread (a mixagem de um corte pode levar alguns décimos de segundo) e guardadas pela chave da fonte (o som do
/// corte muda a chave quando os clipes dele mudam).
#[derive(Default)]
pub(crate) struct TlWaves {
    map: std::collections::HashMap<String, WaveSlot>,
}

impl TlWaves {
    /// Os picos da fonte, se prontos; senão começa (uma vez) a calcular e devolve `None` (o próximo quadro tenta de novo).
    pub(crate) fn get(&mut self, ctx: &egui::Context, p: &Project, blobs: &aiai_core::BlobStore, src: &ItemSource) -> Option<Arc<Vec<f32>>> {
        let key = wave_key(p, src)?;
        if let Some(WaveSlot::Pending(rx)) = self.map.get(&key) {
            match rx.try_recv() {
                Ok(v) => {
                    self.map.insert(key.clone(), WaveSlot::Ready(Arc::new(v)));
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(120));
                    return None;
                }
                Err(_) => {
                    self.map.insert(key.clone(), WaveSlot::Ready(Arc::new(Vec::new())));
                }
            }
        }
        match self.map.get(&key) {
            Some(WaveSlot::Ready(v)) => Some(v.clone()),
            Some(WaveSlot::Pending(_)) => None,
            None => {
                // poucas fontes vivas: esquece as antigas (cortes editados geram chaves novas)
                if self.map.len() > 256 {
                    self.map.clear();
                }
                let (tx, rx) = crossbeam_channel::bounded(1);
                let (p, b, s) = (p.clone(), blobs.clone(), src.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(frame_peaks(&p, &b, &s));
                });
                self.map.insert(key, WaveSlot::Pending(rx));
                ctx.request_repaint_after(Duration::from_millis(120));
                None
            }
        }
    }
}

/// Chave da forma de onda: o asset (imutável) ou o corte com os clipes de som dele naquele momento.
fn wave_key(p: &Project, src: &ItemSource) -> Option<String> {
    match src {
        ItemSource::Audio { asset } => Some(format!("a:{}", asset.0)),
        ItemSource::CutAudio { cut, track } => {
            let (dur, list) = super::audio_ws::cut_audio(p, cut);
            let sig = aiai_3d_render::fnv1a64(serde_json::to_string(&list).unwrap_or_default().as_bytes());
            Some(format!("c:{}:{track:?}:{dur}:{sig:x}", cut.0))
        }
        _ => None,
    }
}

/// Picos (0..1) da fonte: `WAVE_PER_FRAME` por quadro do projeto.
fn frame_peaks(p: &Project, b: &aiai_core::BlobStore, src: &ItemSource) -> Vec<f32> {
    let sr = p.settings.audio_sample_rate.max(8000) as f64;
    let fps = p.settings.fps().max(1.0);
    let buf = match src {
        ItemSource::CutAudio { cut, .. } => aiai_tools::audio::mix_cut(p, b, cut, None),
        ItemSource::Audio { asset } => aiai_tools::audio::asset_audio(p, b, asset).map(|a| aiai_audio::resample::resample(&a, sr as u32)),
        _ => return Vec::new(),
    };
    let Ok(buf) = buf else { return Vec::new() };
    let n = buf.ch.first().map(|c| c.len()).unwrap_or(0);
    let step = (sr / (fps * WAVE_PER_FRAME as f64)).max(1.0);
    let count = (n as f64 / step).ceil() as usize;
    (0..count)
        .map(|k| {
            let (a, z) = ((k as f64 * step) as usize, (((k + 1) as f64 * step) as usize).min(n));
            buf.ch.iter().flat_map(|c| c[a.min(c.len())..z.min(c.len())].iter()).fold(0.0f32, |m, v| m.max(v.abs())).min(1.0)
        })
        .collect()
}

/// Monitor de origem: a fonte aberta, o cabeçote nela e as marcas de entrada/saída.
#[derive(Default)]
pub(crate) struct SourceState {
    pub src: Option<ItemSource>,
    pub name: String,
    pub frame: u32,
    pub mark_in: Option<u32>,
    pub mark_out: Option<u32>,
    pub preview: EditPreview,
    pub play: Option<Play>,
    /// Forma de onda (picos por coluna) da fonte de som aberta, por chave da fonte.
    pub wave: Option<(String, Arc<Vec<f32>>)>,
}

/// Arraste em curso na linha do tempo: a edição de partida e o gesto (o resultado é refeito a cada quadro).
#[derive(Clone)]
pub(crate) struct TlDrag {
    pub kind: DragKind,
    pub start_doc: Arc<EditDoc>,
    pub origin_frame: f32,
    pub origin_y: f32,
    pub cur: Option<Arc<EditDoc>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DragKind {
    Move { ids: Vec<u32>, primary: u32 },
    Trim { ids: Vec<u32>, edge: aiai_tools::edit::ops::Edge, ripple: bool, primary: u32 },
    Roll { left: u32 },
    Slip { ids: Vec<u32> },
    Slide { id: u32 },
    Transition { item: u32, edge: aiai_tools::edit::ops::Edge },
    Pan { scroll0: f32, scroll_y0: f32 },
}

/// Estado do workspace Edição que não mora no documento.
pub(crate) struct EditState {
    pub play: Option<Play>,
    pub sel_edit: Option<NodeId>,
    /// Cabeçote (quadro global da edição).
    pub frame: u32,
    /// Itens selecionados.
    pub sel: Vec<u32>,
    /// Transição selecionada: (item, ponta).
    pub sel_tr: Option<(u32, aiai_tools::edit::ops::Edge)>,
    cache: Option<(u64, NodeId, Arc<EditDoc>, Result<Timeline, String>)>,
    pub preview: EditPreview,
    pub thumbs: CutThumbs,
    /// Formas de onda dos clipes de som na linha do tempo.
    pub waves: TlWaves,
    /// Pixels por quadro na linha do tempo (`None` = ajustar à largura).
    pub zoom: Option<f32>,
    /// Primeiro quadro visível (rolagem horizontal) e rolagem vertical das trilhas (px).
    pub scroll: f32,
    pub scroll_y: f32,
    pub tool: EditTool,
    pub snap: bool,
    /// Seleção vinculada: clicar na imagem seleciona o som do mesmo corte.
    pub linked_sel: bool,
    /// Trilhas de destino (patch de origem) para inserir/sobrescrever: vídeo e áudio.
    pub target_v: Option<u32>,
    pub target_a: Option<u32>,
    pub drag: Option<TlDrag>,
    pub source: SourceState,
    pub focus: Focus,
    /// Transição padrão (Ctrl+D) e duração das transições novas (quadros) e alinhamento.
    pub tr_default: TransitionKind,
    pub tr_frames: u32,
    pub tr_align: TransitionAlign,
    /// Legendas: trilha aberta no painel, limites da verificação e diálogo de arquivo em curso.
    pub cap_track: Option<u32>,
    pub cap_limits: CaptionLimits,
    pub file_job: Option<(FileJob, crossbeam_channel::Receiver<Option<std::path::PathBuf>>)>,
}

/// Diálogo de arquivo das legendas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileJob {
    ImportCaptions,
    ExportSrt,
    ExportVtt,
}

impl Default for EditState {
    fn default() -> Self {
        EditState {
            play: None,
            sel_edit: None,
            frame: 0,
            sel: Vec::new(),
            sel_tr: None,
            cache: None,
            preview: EditPreview::default(),
            thumbs: CutThumbs::default(),
            waves: TlWaves::default(),
            zoom: None,
            scroll: 0.0,
            scroll_y: 0.0,
            tool: EditTool::Select,
            snap: true,
            linked_sel: true,
            target_v: None,
            target_a: None,
            drag: None,
            source: SourceState::default(),
            focus: Focus::Program,
            tr_default: TransitionKind::Dissolve,
            tr_frames: 12,
            tr_align: TransitionAlign::Center,
            cap_track: None,
            cap_limits: CaptionLimits::default(),
            file_job: None,
        }
    }
}

impl EditState {
    pub(crate) fn stop_play(&mut self) {
        if let Some(p) = self.play.take() {
            p.stop();
        }
    }

    pub(crate) fn stop_source(&mut self) {
        if let Some(p) = self.source.play.take() {
            p.stop();
        }
    }

    fn clear_selection(&mut self) {
        self.sel.clear();
        self.sel_tr = None;
    }
}

/// Abre/corrige o documento de edição selecionado e devolve-o com a linha do tempo.
pub(crate) fn sync(env: &mut Env) -> Option<Opened> {
    let docs = actions::docs_of(&env.core.project, DocKind::Edit);
    let st = &mut env.ed.edit;
    let id = match &st.sel_edit {
        Some(c) if docs.iter().any(|(i, _)| i == c) => c.clone(),
        _ => {
            let first = docs.first()?.0.clone();
            st.sel_edit = Some(first.clone());
            st.clear_selection();
            first
        }
    };
    let core = &*env.core;
    let fresh = st.cache.as_ref().is_some_and(|(c, i, _, _)| *c == core.change_counter && *i == id);
    if !fresh {
        let fps = core.project.settings.fps() as f32;
        let (doc, tl) = match load_edit(&core.project, &id) {
            Ok(d) => {
                let tl = d.timeline(&core.project, fps).map_err(|e| e.to_string());
                (d, tl)
            }
            Err(e) => (EditDoc::with_default_tracks(), Err(e.to_string())),
        };
        st.cache = Some((core.change_counter, id.clone(), Arc::new(doc), tl));
    }
    let (_, _, doc, tl) = st.cache.as_ref()?;
    let (doc, tl) = (doc.clone(), tl.clone());
    st.sel.retain(|i| doc.item(*i).is_some());
    if st.sel_tr.is_some_and(|(i, _)| doc.item(i).is_none()) {
        st.sel_tr = None;
    }
    // trilhas de destino padrão: a primeira de cada tipo
    let first = |k: TrackKind| doc.tracks_of(k).first().map(|t| t.id);
    if st.target_v.is_none_or(|t| doc.track(t).is_none()) {
        st.target_v = first(TrackKind::Video);
    }
    if st.target_a.is_none_or(|t| doc.track(t).is_none()) {
        st.target_a = first(TrackKind::Audio);
    }
    if st.cap_track.is_none_or(|t| doc.track(t).is_none_or(|x| x.kind != TrackKind::Subtitle)) {
        st.cap_track = first(TrackKind::Subtitle);
    }
    if let Ok(l) = &tl {
        st.frame = st.frame.min(l.total.saturating_sub(1).max(0));
    }
    Some(Opened { id, doc, tl })
}

/// Regrava a edição (`EDIT_SET` com `id`): um passo de desfazer.
pub(crate) fn save(env: &mut Env, id: &NodeId, doc: &EditDoc) -> bool {
    let mut params = serde_json::to_value(doc).unwrap_or(Value::Null);
    if let Some(o) = params.as_object_mut() {
        o.insert("id".into(), json!(id));
    }
    let r = actions::run(env.core, "EDIT_SET", params);
    actions::notify_err(env.ed, r).is_some()
}

/// Muda a edição aberta com `f` (numa cópia) e grava se `f` disser que mudou.
pub(crate) fn modify(env: &mut Env, f: impl FnOnce(&mut EditDoc, &Project) -> bool) -> bool {
    let Some(o) = sync(env) else { return false };
    let mut d = (*o.doc).clone();
    if f(&mut d, &env.core.project) {
        return save(env, &o.id, &d);
    }
    false
}

/// Duração da mídia de uma fonte no projeto (corte, som com `duration_s`).
pub(crate) fn media_of(p: &Project) -> impl Fn(&ItemSource) -> Option<u32> + '_ {
    move |s| media_frames(p, s)
}

/// Cortes do projeto na ordem do documento: (id, código "C003", nome da cena, duração).
pub(crate) fn project_cuts(p: &Project) -> Vec<(NodeId, String, String, u32)> {
    let mut out = Vec::new();
    for ep in &p.roots.episodes {
        let Some(epn) = p.node(ep) else { continue };
        for sc in &epn.children {
            let Some(scn) = p.node(sc) else { continue };
            for c in &scn.children {
                if let Some(NodeKind::Cut(cd)) = p.node(c).map(|n| &n.data) {
                    out.push((c.clone(), format!("C{:03}", cd.number), scn.name.clone(), cd.duration));
                }
            }
        }
    }
    out
}

/// Sons e imagens do projeto (assets): (id, nome, é som, duração em quadros se souber).
pub(crate) fn project_assets(p: &Project) -> Vec<(NodeId, String, bool, Option<u32>)> {
    let mut v: Vec<(NodeId, String, bool, Option<u32>)> = p
        .nodes
        .values()
        .filter_map(|n| match &n.data {
            NodeKind::Asset(a) if a.asset == AssetType::AudioClip => Some((n.id.clone(), n.name.clone(), true, media_frames(p, &ItemSource::Audio { asset: n.id.clone() }))),
            NodeKind::Asset(a) if matches!(a.asset, AssetType::Texture | AssetType::Background) && a.blob.is_some() => Some((n.id.clone(), n.name.clone(), false, None)),
            _ => None,
        })
        .collect();
    v.sort_by(|a, b| (!a.2, &a.1).cmp(&(!b.2, &b.1)));
    v
}

pub(crate) fn cut_code(p: &Project, cut: &Id) -> String {
    match p.node(cut).map(|n| &n.data) {
        Some(NodeKind::Cut(cd)) => format!("C{:03}", cd.number),
        _ => "?".into(),
    }
}

/// Nome curto da fonte de um item (corte, som, imagem, cor, texto da legenda).
pub(crate) fn source_label(p: &Project, s: &ItemSource) -> String {
    match s {
        ItemSource::Cut { cut } => cut_code(p, cut),
        ItemSource::CutAudio { cut, track } => match track {
            Some(t) => format!("{} · {t:?}", cut_code(p, cut)),
            None => cut_code(p, cut),
        },
        ItemSource::Image { asset } | ItemSource::Audio { asset } => p.node(asset).map(|n| n.name.clone()).unwrap_or_else(|| "?".into()),
        ItemSource::Color { .. } => tr("editing.src.color").to_owned(),
        ItemSource::Caption { text, .. } => actions::excerpt(text, 40),
    }
}

/// "m:ss.ff" (segundos com centésimos).
pub(crate) fn fmt_time(frames: u32, fps: f64) -> String {
    let s = frames as f64 / fps.max(1.0);
    format!("{}:{:05.2}", (s / 60.0).floor() as u32, s % 60.0)
}

/// Timecode "hh:mm:ss:ff".
pub(crate) fn timecode(frames: u32, fps: f64) -> String {
    let nominal = fps.round().max(1.0) as u32;
    let (ff, s) = (frames % nominal, frames / nominal);
    format!("{:02}:{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60, ff)
}

// ------------------------------------------------------------------------------------------------ Mídia

/// Fonte arrastada da Mídia (ou do monitor de origem) para a linha do tempo.
#[derive(Clone, Debug)]
pub(crate) struct MediaDrag {
    pub src: ItemSource,
    pub src_in: u32,
    pub dur: u32,
    pub label: String,
}

/// Abre a fonte no monitor de origem (marcas limpas).
pub(crate) fn open_source(env: &mut Env, src: ItemSource, name: String) {
    let st = &mut env.ed.edit;
    st.stop_source();
    st.source.src = Some(src);
    st.source.name = name;
    st.source.frame = 0;
    st.source.mark_in = None;
    st.source.mark_out = None;
    st.focus = Focus::Source;
}

pub(crate) fn media(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let docs = actions::docs_of(&env.core.project, DocKind::Edit);
    let opened = sync(env);
    let name = opened.as_ref().and_then(|o| docs.iter().find(|(i, _)| *i == o.id)).map(|(_, n)| n.clone()).unwrap_or_else(|| tr("editing.no_edit").to_owned());
    let cb = egui::ComboBox::from_id_salt("edit_doc").width((ui.available_width() - 8.0).max(60.0)).selected_text(name).show_ui(ui, |ui| {
        for (id, n) in &docs {
            if ui.selectable_label(opened.as_ref().map(|o| &o.id) == Some(id), n).clicked() {
                env.ed.edit.sel_edit = Some(id.clone());
                env.ed.edit.clear_selection();
            }
        }
    });
    env.note("edit:doc_picker", cb.response.rect);
    let cuts = project_cuts(&env.core.project);
    ui.horizontal_wrapped(|ui| {
        let b = ui.add_enabled(!cuts.is_empty(), egui::Button::new(tr("editing.new_edit")));
        env.note("edit:new_edit", b.rect);
        if b.on_hover_text(tr("editing.new_edit.tip")).clicked() {
            let name = actions::next_doc_name(&env.core.project, DocKind::Edit, tr("editing.default_name"));
            // todos os cortes em sequência: imagem em V1 e som vinculado em A1
            let mut doc = EditDoc::with_default_tracks();
            let (v, a) = (doc.tracks_of(TrackKind::Video)[0].id, doc.tracks_of(TrackKind::Audio)[0].id);
            let mut at = 0;
            for (c, _, _, dur) in &cuts {
                let items = EditDoc::source_items(&ItemSource::Cut { cut: c.clone() }, 0, *dur, at, Some(v), Some(a));
                doc.overwrite_items(items);
                at += dur;
            }
            doc.credits_seconds = 6.0;
            let mut params = serde_json::to_value(&doc).unwrap_or(Value::Null);
            params["name"] = json!(name);
            let r = actions::run(env.core, "EDIT_SET", params);
            if let Some(v) = actions::notify_err(env.ed, r) {
                env.ed.edit.sel_edit = v.get("edit").and_then(Value::as_str).map(|s| Id(s.to_owned()));
                env.ed.edit.frame = 0;
            }
        }
    });
    ui.add_space(2.0);
    dim_label(ui, &t, tr("editing.media_hint"));
    let ctx = ui.ctx().clone();
    let mut budget = THUMB_BUDGET;
    let fps = env.core.project.settings.fps();
    let assets = project_assets(&env.core.project);
    egui::ScrollArea::vertical().id_salt("edit_media").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        if cuts.is_empty() {
            ui.label(egui::RichText::new(tr("panel.media.empty")).color(t.text_dim));
        }
        let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
        for (i, (cut, code, scene, dur)) in cuts.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), Sense::click_and_drag());
            env.note(format!("edit:media:{i}"), rect);
            let painter = ui.painter().clone();
            if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            if env.ed.edit.source.src.as_ref().and_then(|s| s.cut()) == Some(cut) {
                painter.rect_stroke(rect, 4.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
            }
            let th = 36.0;
            let thumb = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 4.0), egui::vec2((th * w / h).min(72.0), th));
            painter.rect_filled(thumb, 2.0, Color32::WHITE);
            if let Some(tex) = env.ed.edit.thumbs.get(&ctx, env.core, cut, &mut budget) {
                painter.image(tex, thumb, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            let clip = Rect::from_min_max(egui::pos2(thumb.right() + 8.0, rect.top()), rect.max);
            let tp = painter.with_clip_rect(clip);
            tp.text(egui::pos2(clip.left(), rect.top() + 6.0), egui::Align2::LEFT_TOP, format!("{code}  ·  {:.1} s", *dur as f64 / fps), FontId::proportional(12.5), t.text);
            tp.text(egui::pos2(clip.left(), rect.top() + 24.0), egui::Align2::LEFT_TOP, scene, FontId::proportional(11.0), t.text_dim);
            let resp = resp.on_hover_text(tr("editing.media_add.tip"));
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(&ctx, MediaDrag { src: ItemSource::Cut { cut: cut.clone() }, src_in: 0, dur: *dur, label: code.clone() });
            }
            if resp.double_clicked() {
                open_source(env, ItemSource::Cut { cut: cut.clone() }, format!("{code} · {scene}"));
            }
        }
        if !assets.is_empty() {
            section(ui, &t, "editing.media_assets");
        }
        for (k, (id, name, sound, dur)) in assets.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), Sense::click_and_drag());
            env.note(format!("edit:asset:{k}"), rect);
            let painter = ui.painter().clone();
            if resp.hovered() {
                painter.rect_filled(rect, 4.0, t.bg_hover);
            }
            let badge = Rect::from_min_size(rect.left_top() + egui::vec2(4.0, 5.0), egui::vec2(14.0, 14.0));
            painter.rect_filled(badge, 3.0, if *sound { Color32::from_rgb(70, 150, 100) } else { Color32::from_rgb(140, 100, 170) });
            let label = match dur {
                Some(d) => format!("{name}  ·  {:.1} s", *d as f64 / fps),
                None => name.clone(),
            };
            painter.with_clip_rect(rect).text(egui::pos2(badge.right() + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, label, FontId::proportional(11.5), t.text);
            let src = if *sound { ItemSource::Audio { asset: id.clone() } } else { ItemSource::Image { asset: id.clone() } };
            let d = dur.unwrap_or((fps * 3.0).round() as u32).max(1);
            if resp.drag_started() {
                egui::DragAndDrop::set_payload(&ctx, MediaDrag { src: src.clone(), src_in: 0, dur: d, label: name.clone() });
            }
            if resp.double_clicked() {
                open_source(env, src, name.clone());
            }
        }
    });
    let alive: std::collections::HashSet<&str> = cuts.iter().map(|c| c.0.0.as_str()).collect();
    env.ed.edit.thumbs.retain(|k| alive.contains(k));
}

/// Linha do tempo da Edição (em `edit_timeline`).
pub(crate) fn timeline(env: &mut Env, ui: &mut Ui, body: Rect) {
    super::edit_timeline::show(env, ui, body)
}

// ------------------------------------------------------------------------------------------------ Monitor do programa

/// Imagem no tamanho do projeto dentro de `area`, com a textura dada (preto enquanto não houver).
pub(crate) fn paint_monitor(env: &mut Env, ui: &Ui, area: Rect, tex: Option<&egui::TextureHandle>, note: &str) -> Rect {
    let t = env.t;
    let (w, h) = (env.core.project.settings.width.max(1) as f32, env.core.project.settings.height.max(1) as f32);
    let s = (area.width() / w).min(area.height() / h).max(0.01);
    let img_rect = Rect::from_center_size(area.center(), egui::vec2(w * s, h * s));
    let painter = ui.painter();
    painter.rect_filled(img_rect, 0.0, Color32::BLACK);
    if let Some(tex) = tex {
        painter.image(tex.id(), img_rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    painter.rect_stroke(img_rect, 0.0, Stroke::new(1.0, t.border), StrokeKind::Outside);
    env.note(note, img_rect);
    img_rect
}

pub(crate) fn preview(env: &mut Env, ui: &mut Ui, body: Rect) {
    let t = env.t;
    let mut child = body_ui(ui, body, 6.0);
    let ui = &mut child;
    let Some(o) = sync(env) else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr("editing.no_edit_long")).color(t.text_dim)));
        return;
    };
    let tl = match &o.tl {
        Ok(l) => *l,
        Err(e) => {
            ui.label(egui::RichText::new(format!("{}: {e}", tr("editing.invalid"))).color(t.accent));
            return;
        }
    };
    let fps = env.core.project.settings.fps();
    let ctx = ui.ctx().clone();
    let last = tl.total.saturating_sub(1);
    // relógio da reprodução (o som define o começo)
    if let Some(p) = env.ed.edit.play.as_mut() {
        match p.tick(fps, env.core.project.settings.audio_sample_rate, last) {
            Some(f) => env.ed.edit.frame = f,
            None => {
                env.ed.edit.frame = if p.rate < 0.0 { 0 } else { last };
                env.ed.edit.stop_play();
            }
        }
        ctx.request_repaint();
    }
    let frame = env.ed.edit.frame;
    // durante um arraste na linha do tempo, o monitor mostra a edição em curso
    let doc = env.ed.edit.drag.as_ref().and_then(|d| d.cur.clone()).unwrap_or_else(|| o.doc.clone());
    let key = format!("{}:{}", o.id.0, env.ed.edit.drag.as_ref().map(|d| d.cur.as_ref().map(|c| Arc::as_ptr(c) as usize).unwrap_or(0)).unwrap_or(0));
    env.ed.edit.preview.update(&ctx, env.core, key, &doc, frame);

    let bar_h = 30.0;
    let title_h = 16.0;
    let full = ui.max_rect();
    ui.painter().text(full.left_top(), egui::Align2::LEFT_TOP, format!("{}  ·  {}", tr("editing.program"), timecode(frame, fps)), FontId::proportional(11.0), if env.ed.edit.focus == Focus::Program { t.accent } else { t.text_dim });
    let area = Rect::from_min_max(egui::pos2(full.left(), full.top() + title_h), egui::pos2(full.right(), full.bottom() - bar_h - 4.0));
    let tex = env.ed.edit.preview.texture.clone();
    let img_rect = paint_monitor(env, ui, area, tex.as_ref(), "edit:preview_image");
    let resp = ui.interact(area, EId::new("edit_program_area"), Sense::click());
    if resp.clicked() {
        env.ed.edit.focus = Focus::Program;
    }
    let painter = ui.painter();
    if let Some(e) = &env.ed.edit.preview.error {
        painter.text(img_rect.center(), egui::Align2::CENTER_CENTER, e, FontId::proportional(12.0), t.accent);
    } else if env.ed.edit.preview.shown_frame != Some(frame) {
        painter.text(img_rect.right_top() + egui::vec2(-8.0, 8.0), egui::Align2::RIGHT_TOP, tr("editing.rendering"), FontId::proportional(11.0), Color32::from_gray(200));
    }

    let bar = Rect::from_min_max(egui::pos2(full.left(), full.bottom() - bar_h), full.max);
    let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let mut f = frame;
    let playing = env.ed.edit.play.is_some();
    let play = env.icon_button(&mut bui, if playing { crate::icons::IconId::Pause } else { crate::icons::IconId::Play }, if playing { "timeline.pause" } else { "timeline.play" }, playing, tl.total > 1, "edit:play");
    if play.clicked() {
        toggle_program_play(env, &o, 1.0);
        f = env.ed.edit.frame;
    }
    let prev = env.icon_button(&mut bui, crate::icons::IconId::StepBack, "editing.prev", false, f > 0, "edit:prev");
    let next = env.icon_button(&mut bui, crate::icons::IconId::StepForward, "editing.next", false, f < last, "edit:next");
    if prev.clicked() {
        f = f.saturating_sub(1);
    }
    if next.clicked() {
        f = (f + 1).min(last);
    }
    let slider_w = (bui.available_width() - 190.0).max(60.0);
    bui.spacing_mut().slider_width = slider_w;
    let r = bui.add(egui::Slider::new(&mut f, 0..=last.max(1)).show_value(false));
    env.note("edit:scrub", r.rect);
    if (r.changed() || prev.clicked() || next.clicked()) && env.ed.edit.play.is_some() {
        env.ed.edit.stop_play();
    }
    bui.label(egui::RichText::new(format!("{}  /  {}   ·   {} {}", fmt_time(f, fps), fmt_time(tl.total, fps), tr("editing.frame_short"), f + 1)).size(12.0).color(t.text_dim));
    env.ed.edit.frame = f.min(last);
}

/// Liga/desliga a reprodução do programa (com o som da edição em segundo plano). `rate` 1 = normal (J/K/L mudam).
pub(crate) fn toggle_program_play(env: &mut Env, o: &Opened, rate: f64) {
    if env.ed.edit.play.is_some() && rate == 1.0 {
        env.ed.edit.stop_play();
        return;
    }
    env.ed.edit.stop_play();
    env.ed.edit.stop_source();
    let last = o.tl.as_ref().map(|t| t.total.saturating_sub(1)).unwrap_or(0);
    let f = env.ed.edit.frame;
    let start = if rate > 0.0 && f >= last { 0 } else { f };
    let (project, blobs, doc) = (env.core.project.clone(), env.core.blobs.clone(), o.doc.clone());
    let sound = env.ed.live.enabled();
    env.ed.edit.play = Some(Play::start(start, rate, sound, move || aiai_tools::edit::mix_edit(&project, &blobs, &doc).map_err(|e| e.to_string())));
    env.ed.edit.frame = start;
}

// ------------------------------------------------------------------------------------------------ Propriedades

pub(crate) fn properties(env: &mut Env, ui: &mut Ui) {
    let t = env.t;
    let Some(o) = sync(env) else {
        dim_label(ui, &t, tr("editing.no_edit_long"));
        return;
    };
    let fps = env.core.project.settings.fps();
    let mut doc = (*o.doc).clone();
    let mut changed = false;
    let media = |s: &ItemSource| media_frames(&env.core.project, s);
    let sel = env.ed.edit.sel.first().copied().filter(|i| doc.item(*i).is_some());
    if let Some((iid, edge)) = env.ed.edit.sel_tr.filter(|(i, _)| doc.item(*i).is_some()) {
        section(ui, &t, "editing.transition");
        let it = doc.item(iid).expect("existe").clone();
        let cur = match edge {
            aiai_tools::edit::ops::Edge::In => it.transition_in.clone(),
            aiai_tools::edit::ops::Edge::Out => it.transition_out.clone(),
        };
        if let Some(cur) = cur {
            let mut next = cur.clone();
            egui::Grid::new("edit_tr_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                dim_label(ui, &t, tr("editing.transition"));
                let r = egui::ComboBox::from_id_salt(("edit_tr_kind", iid)).width(150.0).selected_text(tr(&transition_key(next.kind))).show_ui(ui, |ui| {
                    for k in TransitionKind::ALL.into_iter().filter(|k| *k != TransitionKind::Cut) {
                        ui.selectable_value(&mut next.kind, k, tr(&transition_key(k)));
                    }
                });
                env.note("edit:props:transition", r.response.rect);
                ui.end_row();
                dim_label(ui, &t, tr("editing.transition_frames"));
                if let Some(v) = commit_on_release(ui, EId::new(("edit_trf", iid, &o.id.0)), cur.frames, |ui, v| ui.add(egui::DragValue::new(v).range(1..=240).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
                    next.frames = v;
                }
                ui.end_row();
                dim_label(ui, &t, tr("editing.tr_align"));
                ui.horizontal(|ui| {
                    for (a, k) in [(TransitionAlign::Start, "editing.tr_align.start"), (TransitionAlign::Center, "editing.tr_align.center"), (TransitionAlign::End, "editing.tr_align.end")] {
                        let r = ui.selectable_label(next.align == a, tr(k));
                        env.note(format!("edit:props:align:{}", a as u8), r.rect);
                        if r.clicked() {
                            next.align = a;
                        }
                    }
                });
                ui.end_row();
            });
            dim_label(ui, &t, &format!("{} {:.2} s", tr("editing.duration"), next.frames as f64 / fps));
            if next != cur {
                doc.set_transition(iid, edge, Some(next));
                changed = true;
            }
            let del = env.icon_button(ui, crate::icons::IconId::Trash, "editing.tr_delete", false, true, "edit:tr_delete");
            if del.clicked() {
                doc.set_transition(iid, edge, None);
                env.ed.edit.sel_tr = None;
                changed = true;
            }
        }
    } else if let Some(id) = sel {
        let it = doc.item(id).expect("existe").clone();
        section(ui, &t, "editing.clip");
        ui.label(egui::RichText::new(format!("{}  ·  {}", source_label(&env.core.project, &it.source), doc.track_label(it.track))).strong());
        let mlen = media(&it.source);
        egui::Grid::new("edit_item_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            dim_label(ui, &t, tr("editing.start"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_is", id, &o.id.0)), it.start, |ui, v| ui.add(egui::DragValue::new(v).speed(0.3))) {
                let ids = if env.ed.edit.linked_sel { doc.linked(id) } else { vec![id] };
                if let Err(e) = doc.move_items(&ids, it.source.track_kind(), v as i64 - it.start as i64, 0) {
                    env.ed.toast = Some(e);
                } else {
                    changed = true;
                }
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.in"));
            let max_in = mlen.map(|m| m.saturating_sub(1)).unwrap_or(u32::MAX / 4);
            if let Some(v) = commit_on_release(ui, EId::new(("edit_in", id, &o.id.0)), it.src_in, |ui, v| ui.add(egui::DragValue::new(v).range(0..=max_in).speed(0.3))) {
                // vinculados andam juntos, como no Início (auditoria F01): vídeo e áudio continuam em sincronia
                let ids = if env.ed.edit.linked_sel { doc.linked(id) } else { vec![id] };
                doc.slip(&ids, v as i64 - it.src_in as i64, &media);
                changed = true;
            }
            ui.end_row();
            dim_label(ui, &t, tr("editing.duration"));
            if let Some(v) = commit_on_release(ui, EId::new(("edit_dur", id, &o.id.0)), it.dur, |ui, v| ui.add(egui::DragValue::new(v).range(1..=100_000).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
                let ids = if env.ed.edit.linked_sel { doc.linked(id) } else { vec![id] };
                doc.trim(&ids, aiai_tools::edit::ops::Edge::Out, v as i64 - it.dur as i64, false, &media);
                changed = true;
            }
            ui.end_row();
            if let Some(a) = it.anchor {
                dim_label(ui, &t, tr("editing.anchor"));
                ui.label(doc.item(a).map(|x| source_label(&env.core.project, &x.source)).unwrap_or_default());
                ui.end_row();
            }
        });
        dim_label(ui, &t, &format!("{} – {}  ({:.2} s)", timecode(it.start, fps), timecode(it.end(), fps), it.dur as f64 / fps));
        ui.horizontal_wrapped(|ui| {
            if it.link.is_some() {
                let b = ui.button(tr("editing.unlink"));
                env.note("edit:props:unlink", b.rect);
                if b.on_hover_text(tr("editing.unlink.tip")).clicked() {
                    doc.unlink(&[id]);
                    changed = true;
                }
            } else if env.ed.edit.sel.len() > 1 {
                let b = ui.button(tr("editing.link"));
                env.note("edit:props:link", b.rect);
                if b.clicked() {
                    doc.link(&env.ed.edit.sel.clone());
                    changed = true;
                }
            }
            let en = ui.checkbox(&mut it.enabled.clone(), tr("editing.enabled"));
            if en.clicked() {
                if let Some(x) = doc.item_mut(id) {
                    x.enabled = !x.enabled;
                }
                changed = true;
            }
            let del = env.icon_button(ui, crate::icons::IconId::Trash, "editing.clip_delete", false, true, "edit:clip_delete");
            if del.clicked() {
                let ids = if env.ed.edit.linked_sel { doc.with_linked(&env.ed.edit.sel) } else { env.ed.edit.sel.clone() };
                doc.delete_items(&ids, false);
                env.ed.edit.sel.clear();
                changed = true;
            }
        });
        dim_label(ui, &t, tr("editing.props_effects_hint"));
    } else {
        dim_label(ui, &t, tr("editing.props_hint"));
    }

    // ---- edição inteira: créditos e fade final, num bloco com escopo explícito (auditoria UX-11): valem para a
    // sequência toda, não para o clipe escolhido acima
    ui.add_space(8.0);
    ui.separator();
    ui.label(egui::RichText::new(tr("editing.scope_sequence")).strong().size(13.0));
    dim_label(ui, &t, tr("editing.scope_sequence.tip"));
    section(ui, &t, "editing.credits");
    let w = (ui.available_width() - 4.0).max(80.0);
    let (r, c) = super::text_commit_multi(ui, EId::new(("edit_credits", &o.id.0)), &doc.credits.join("\n"), w, tr("editing.credits_hint"));
    env.note("edit:props:credits", r.rect);
    if let Some(x) = c {
        doc.credits = x.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect();
        changed = true;
    }
    egui::Grid::new("edit_doc_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        dim_label(ui, &t, tr("editing.credits_seconds"));
        if let Some(v) = commit_on_release(ui, EId::new(("edit_cs", &o.id.0)), doc.credits_seconds, |ui, v| ui.add(egui::DragValue::new(v).range(1.0..=60.0).speed(0.1).suffix(" s"))) {
            doc.credits_seconds = v;
            changed = true;
        }
        ui.end_row();
        dim_label(ui, &t, tr("editing.fade_out"));
        if let Some(v) = commit_on_release(ui, EId::new(("edit_fo", &o.id.0)), doc.fade_out_end, |ui, v| ui.add(egui::DragValue::new(v).range(0..=240).speed(0.3).suffix(format!(" {}", tr("unit.frames_short"))))) {
            doc.fade_out_end = v;
            changed = true;
        }
        ui.end_row();
    });
    if changed {
        save(env, &o.id, &doc);
    }
}

/// Transição nova com os ajustes do painel Transições.
pub(crate) fn new_transition(env: &Env, kind: TransitionKind) -> Transition {
    Transition { kind, frames: env.ed.edit.tr_frames.max(1), align: env.ed.edit.tr_align }
}

/// Itens novos de uma fonte (imagem/som vinculados) nas trilhas de destino.
pub(crate) fn items_for(env: &Env, src: &ItemSource, src_in: u32, dur: u32, at: u32) -> Vec<EditItem> {
    EditDoc::source_items(src, src_in, dur, at, env.ed.edit.target_v, env.ed.edit.target_a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::doc::AUTHOR;
    use aiai_tools::edit::{EditClip, SubtitleEntry};

    fn session_with_two_cuts() -> (CoreSession, NodeId, NodeId) {
        let mut core = CoreSession::for_tests("t");
        core.author = AUTHOR.into();
        core.project.settings.width = 64;
        core.project.settings.height = 36;
        let a = crate::app::commands::quick_start(&mut core, 24, None).unwrap();
        let scene = core.project.node(&a).and_then(|n| n.parent.clone()).unwrap();
        let b = crate::app::commands::add_cut(&mut core, &scene, 12, None).unwrap();
        (core, a, b)
    }

    #[test]
    fn edit_set_v1_is_converted_and_round_trips_through_the_bridge_with_undo() {
        let (mut core, a, b) = session_with_two_cuts();
        let doc = EditDoc {
            clips: vec![EditClip { cut: a.clone(), in_frame: 0, out_frame: None, transition_in: None }, EditClip { cut: b.clone(), in_frame: 2, out_frame: Some(10), transition_in: Some(Transition::new(TransitionKind::Dissolve, 4)) }],
            subtitles: vec![SubtitleEntry { start_frame: 1, end_frame: 5, text: "oi".into() }],
            credits_seconds: 6.0,
            ..Default::default()
        };
        let mut params = serde_json::to_value(&doc).unwrap();
        params["name"] = json!("Edição");
        let v = actions::run(&mut core, "EDIT_SET", params).unwrap();
        let id = Id(v["edit"].as_str().unwrap().to_owned());
        assert_eq!(actions::docs_of(&core.project, DocKind::Edit).len(), 1);
        let back = load_edit(&core.project, &id).unwrap();
        assert_eq!(back.version, 2);
        let vt = back.tracks_of(TrackKind::Video)[0].id;
        let pics: Vec<(u32, u32, u32)> = back.items_on(vt).iter().map(|i| (i.start, i.dur, i.src_in)).collect();
        assert_eq!(pics, vec![(0, 24, 0), (24, 4, 6)], "o dissolve de 4 vira transição no fim do corte, com a alça do 2º");
        assert_eq!(back.timeline(&core.project, 24.0).unwrap().credits_start, 28);
        // um item que sobrepõe outro na mesma trilha é recusado e não muda o documento
        let mut bad = back.clone();
        let mut x = bad.items_on(vt)[0].clone();
        x.id = 999;
        x.start = 3;
        bad.items.push(x);
        let mut p = serde_json::to_value(&bad).unwrap();
        p["id"] = json!(id);
        assert!(actions::run(&mut core, "EDIT_SET", p).is_err());
        assert_eq!(load_edit(&core.project, &id).unwrap(), back);
        core.undo().unwrap();
        assert!(actions::docs_of(&core.project, DocKind::Edit).is_empty());
    }

    #[test]
    fn project_cuts_follow_document_order_and_transition_keys_exist() {
        let (core, a, b) = session_with_two_cuts();
        let cuts = project_cuts(&core.project);
        assert_eq!(cuts.iter().map(|c| c.0.clone()).collect::<Vec<_>>(), vec![a, b]);
        use crate::i18n::{Lang, tr_in};
        for k in TransitionKind::ALL {
            let key = transition_key(k);
            assert_ne!(tr_in(Lang::En, &key), key, "{key}");
            assert_ne!(tr_in(Lang::Pt, &key), key, "{key}");
        }
        for tool in EditTool::ALL {
            let key = format!("editing.tool.{}", tool.key());
            assert_ne!(tr_in(Lang::En, &key), key, "{key}");
        }
        assert_eq!(fmt_time(36, 24.0), "0:01.50");
        assert_eq!(timecode(24 * 61 + 5, 24.0), "00:01:01:05");
    }
}
