//! Documento de edição v2 (S24 3.13, estilo Premiere): trilhas separadas de vídeo (V1..Vn), áudio (A1..An) e legenda
//! (S1..Sn); itens que apontam para cortes, imagens, cor sólida, o áudio de um corte, assets de áudio ou texto de
//! legenda, com entrada na mídia, velocidade, opacidade, volume/balanço, filtros (`aiai-fx`), rack de áudio
//! (`aiai-audio`), chaves, vínculo A/V e transições nas pontas. Legendas podem seguir um clipe (`anchor`).
//!
//! Tempo sempre em quadros do projeto. Regra das trilhas: itens da mesma trilha não se sobrepõem; a transição entre dois
//! itens vizinhos (um termina onde o outro começa) usa as "alças" da mídia além dos pontos de entrada/saída (quadro
//! congelado quando a mídia acaba, como "repetir quadros" do Premiere).

use super::legacy::{EditClip, SubtitleEntry, overlaps, timeline_v1};
use crate::context::*;
use aiai_core::*;
use aiai_fx::EffectInstance;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const EDIT_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    #[default]
    Cut,
    /// Do preto: o clipe entra vindo do preto (só o clipe que entra; o anterior corta seco).
    FadeBlack,
    /// Do branco.
    FadeWhite,
    /// Dissolver cruzado (cross dissolve).
    Dissolve,
    /// Cortina: a imagem nova entra da direita para a esquerda.
    WipeLeft,
    WipeRight,
    /// Flash branco curto no ponto de corte.
    Flash,
    /// Mergulho no preto: o anterior escurece até o preto e o novo clareia a partir dele.
    DipBlack,
    DipWhite,
    WipeUp,
    WipeDown,
    /// Deslizar: o novo desliza por cima do anterior, vindo da direita.
    SlideLeft,
    SlideRight,
    /// Empurrar: o novo empurra o anterior para fora.
    PushLeft,
    PushRight,
    /// Íris: círculo que abre do centro.
    Iris,
    /// Áudio: cruzamento de potência constante (em itens de áudio, qualquer transição que não seja corte cruza assim).
    Crossfade,
}

impl TransitionKind {
    /// Todas, na ordem do painel Transições.
    pub const ALL: [TransitionKind; 17] = [
        TransitionKind::Cut,
        TransitionKind::Dissolve,
        TransitionKind::DipBlack,
        TransitionKind::DipWhite,
        TransitionKind::FadeBlack,
        TransitionKind::FadeWhite,
        TransitionKind::Flash,
        TransitionKind::WipeLeft,
        TransitionKind::WipeRight,
        TransitionKind::WipeUp,
        TransitionKind::WipeDown,
        TransitionKind::SlideLeft,
        TransitionKind::SlideRight,
        TransitionKind::PushLeft,
        TransitionKind::PushRight,
        TransitionKind::Iris,
        TransitionKind::Crossfade,
    ];

    /// Fade do v1: age só no clipe que entra, a partir do ponto de corte, sem usar o vizinho.
    pub fn is_fade_in(self) -> bool {
        matches!(self, TransitionKind::FadeBlack | TransitionKind::FadeWhite | TransitionKind::Flash)
    }

    pub fn key(self) -> &'static str {
        match self {
            TransitionKind::Cut => "cut",
            TransitionKind::FadeBlack => "fade_black",
            TransitionKind::FadeWhite => "fade_white",
            TransitionKind::Dissolve => "dissolve",
            TransitionKind::WipeLeft => "wipe_left",
            TransitionKind::WipeRight => "wipe_right",
            TransitionKind::Flash => "flash",
            TransitionKind::DipBlack => "dip_black",
            TransitionKind::DipWhite => "dip_white",
            TransitionKind::WipeUp => "wipe_up",
            TransitionKind::WipeDown => "wipe_down",
            TransitionKind::SlideLeft => "slide_left",
            TransitionKind::SlideRight => "slide_right",
            TransitionKind::PushLeft => "push_left",
            TransitionKind::PushRight => "push_right",
            TransitionKind::Iris => "iris",
            TransitionKind::Crossfade => "crossfade",
        }
    }
}

/// Onde a transição fica em relação ao ponto de corte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionAlign {
    /// Metade antes e metade depois do corte (padrão do Premiere).
    #[default]
    Center,
    /// Começa no corte (usa a alça do clipe que sai).
    Start,
    /// Termina no corte (usa a alça do clipe que entra; é como o v1 sobrepunha).
    End,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    pub kind: TransitionKind,
    /// Duração em quadros.
    pub frames: u32,
    #[serde(default, skip_serializing_if = "is_center")]
    pub align: TransitionAlign,
}

fn is_center(a: &TransitionAlign) -> bool {
    *a == TransitionAlign::Center
}

impl Transition {
    pub fn new(kind: TransitionKind, frames: u32) -> Self {
        Transition { kind, frames, align: TransitionAlign::Center }
    }

    /// (quadros antes do corte, quadros depois) de uma transição entre dois vizinhos.
    pub fn split(&self) -> (u32, u32) {
        if self.kind.is_fade_in() {
            return (0, self.frames);
        }
        match self.align {
            TransitionAlign::Center => (self.frames / 2, self.frames - self.frames / 2),
            TransitionAlign::Start => (0, self.frames),
            TransitionAlign::End => (self.frames, 0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    #[default]
    Video,
    Audio,
    Subtitle,
}

/// Estilo de legenda (frações do quadro). O padrão é a legenda queimada do v1 (corpo 9 % da altura, branco com
/// contorno escuro ~4 px em 1080p, quebra em 86 % da largura, base em 91 %).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct CaptionStyle {
    /// Corpo do texto em fração da altura.
    pub size: f32,
    pub color: [f32; 4],
    /// Contorno (cor); `None` = sem contorno.
    pub outline: Option<[f32; 4]>,
    /// Espessura do contorno em fração da altura (mínimo 1,5 px).
    pub outline_width: f32,
    /// Caixa atrás do texto (RGBA); `None` = sem caixa.
    pub background: Option<[f32; 4]>,
    /// Base da última linha em fração da altura (0 = topo, 1 = base).
    pub y: f32,
    /// Largura máxima da linha (quebra automática) em fração da largura.
    pub max_width: f32,
    /// Entrelinha em múltiplos do corpo.
    pub line_spacing: f32,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        CaptionStyle { size: 0.09, color: [1.0, 1.0, 1.0, 1.0], outline: Some([0.04, 0.04, 0.07, 1.0]), outline_width: 0.0038, background: None, y: 0.91, max_width: 0.86, line_spacing: 1.25 }
    }
}

impl CaptionStyle {
    pub fn validate(&self) -> Result<(), String> {
        let ok = |v: f32, lo: f32, hi: f32| v.is_finite() && v >= lo && v <= hi;
        if !ok(self.size, 0.01, 0.5) || !ok(self.outline_width, 0.0, 0.05) || !ok(self.y, 0.0, 1.0) || !ok(self.max_width, 0.1, 1.0) || !ok(self.line_spacing, 0.5, 4.0) {
            return Err("estilo de legenda fora do intervalo (size 0.01..0.5, outline_width 0..0.05, y 0..1, max_width 0.1..1, line_spacing 0.5..4)".into());
        }
        Ok(())
    }
}

fn yes() -> bool {
    true
}
fn one() -> f32 {
    1.0
}
fn is_one(v: &f32) -> bool {
    *v == 1.0
}
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditTrack {
    /// Identificador estável (os itens apontam para ele).
    pub id: u32,
    pub kind: TrackKind,
    /// Nome mostrado (padrão V1/A1/S1 pela posição).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Trava: nenhuma ferramenta mexe nos itens desta trilha.
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Vídeo/legenda: olho desligado (não aparece no programa nem na exportação).
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// Áudio: mudo.
    #[serde(default, skip_serializing_if = "is_false")]
    pub muted: bool,
    /// Áudio: solo (havendo solo, só as trilhas em solo tocam).
    #[serde(default, skip_serializing_if = "is_false")]
    pub solo: bool,
    /// Áudio: ganho linear da trilha.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f32,
    /// Legenda: idioma (pt, en, ja...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lang: String,
    /// Legenda: queimar na imagem (senão, só a legenda de entrega .srt).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub burn: bool,
    /// Legenda: estilo da trilha (cada legenda pode sobrepor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CaptionStyle>,
}

impl EditTrack {
    pub fn new(id: u32, kind: TrackKind) -> Self {
        EditTrack { id, kind, name: String::new(), locked: false, hidden: false, muted: false, solo: false, volume: 1.0, lang: String::new(), burn: true, style: None }
    }
}

/// O que um item mostra/toca.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemSource {
    /// Imagem de um corte (render do corte).
    Cut { cut: Id },
    /// Som de um corte: a mixagem do corte (com ducking) ou só uma das suas trilhas.
    CutAudio {
        cut: Id,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        track: Option<AudioTrack>,
    },
    /// Imagem importada (asset `texture`/`background` com PNG), ajustada ao quadro.
    Image { asset: Id },
    /// Cor sólida (fundo, cartela).
    Color { rgba: [f32; 4] },
    /// Asset de áudio (WAV).
    Audio { asset: Id },
    /// Legenda (trilhas S).
    Caption {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        speaker: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<CaptionStyle>,
    },
}

impl ItemSource {
    /// Tipo de trilha que aceita esta fonte.
    pub fn track_kind(&self) -> TrackKind {
        match self {
            ItemSource::Cut { .. } | ItemSource::Image { .. } | ItemSource::Color { .. } => TrackKind::Video,
            ItemSource::CutAudio { .. } | ItemSource::Audio { .. } => TrackKind::Audio,
            ItemSource::Caption { .. } => TrackKind::Subtitle,
        }
    }

    pub fn cut(&self) -> Option<&Id> {
        match self {
            ItemSource::Cut { cut } | ItemSource::CutAudio { cut, .. } => Some(cut),
            _ => None,
        }
    }
}

/// Rack de áudio do item (`aiai-audio`); comparado pelo JSON (os efeitos não têm igualdade própria).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct ClipRack(pub aiai_audio::fx::Rack);

impl PartialEq for ClipRack {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(&self.0).ok() == serde_json::to_value(&other.0).ok()
    }
}

impl ClipRack {
    pub fn is_empty(&self) -> bool {
        self.0.slots.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditItem {
    /// Identificador estável do item no documento.
    pub id: u32,
    /// Trilha (`EditTrack.id`).
    pub track: u32,
    /// Quadro da edição onde começa.
    pub start: u32,
    /// Duração na edição (quadros).
    pub dur: u32,
    /// Ponto de entrada na mídia (quadros da mídia).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub src_in: u32,
    pub source: ItemSource,
    /// Velocidade (retime): quadros da mídia por quadro da edição (0.1..10; 1 = normal).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub speed: f32,
    /// Vídeo/legenda: opacidade 0..1.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub opacity: f32,
    /// Áudio: ganho linear (1 = 0 dB).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub volume: f32,
    /// Áudio: balanço -1 (esq.) .. 1 (dir.).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub pan: f32,
    /// Vídeo: filtros (pilha de efeitos do `aiai-fx`: grade, desfoque, vinheta, grão, LUT...), com chaves no tempo do item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<EffectInstance>,
    /// Áudio: rack de efeitos (EQ, compressor, ...).
    #[serde(default, skip_serializing_if = "ClipRack::is_empty")]
    pub audio_fx: ClipRack,
    /// Chaves do item no tempo dele (quadro 0 = início do item): `opacity`, `volume`, `pan`.
    #[serde(default, skip_serializing_if = "KeyTracks::is_empty")]
    pub keys: KeyTracks,
    /// Grupo de vínculo (imagem e som do mesmo corte andam juntos); `None` = solto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<u32>,
    /// Legenda presa a um item: acompanha quando ele anda.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<u32>,
    /// Transição na entrada (com o vizinho que termina onde este começa, ou do nada).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_in: Option<Transition>,
    /// Transição na saída quando não há vizinho depois (para o nada).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_out: Option<Transition>,
    /// Desligado: fica na linha do tempo mas não aparece nem toca.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub enabled: bool,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

impl EditItem {
    pub fn new(id: u32, track: u32, start: u32, dur: u32, source: ItemSource) -> Self {
        EditItem {
            id,
            track,
            start,
            dur,
            src_in: 0,
            source,
            speed: 1.0,
            opacity: 1.0,
            volume: 1.0,
            pan: 0.0,
            filters: Vec::new(),
            audio_fx: ClipRack::default(),
            keys: KeyTracks::default(),
            link: None,
            anchor: None,
            transition_in: None,
            transition_out: None,
            enabled: true,
        }
    }

    pub fn end(&self) -> u32 {
        self.start + self.dur
    }

    /// Quadro da mídia mostrado no quadro `f` da edição (pode cair antes da entrada: alça).
    pub fn src_frame(&self, f: i64) -> i64 {
        self.src_in as i64 + ((f - self.start as i64) as f64 * self.speed as f64).floor() as i64
    }

    /// Fim (exclusivo) do trecho usado na mídia.
    pub fn src_out(&self) -> u32 {
        self.src_in + (self.dur as f64 * self.speed as f64).ceil() as u32
    }

    pub fn caption_text(&self) -> Option<&str> {
        match &self.source {
            ItemSource::Caption { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Valor da chave `prop` no quadro local, multiplicado pelo valor base.
    pub fn keyed(&self, prop: &str, base: f32, local: f32) -> f32 {
        self.keys.eval(prop, local).map(|k| if prop == "pan" { k } else { base * k }).unwrap_or(base)
    }
}

/// Marcador da linha do tempo (o M dos editores): quadro global e nome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditMarker {
    pub frame: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// Documento de edição v2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditDoc {
    /// Versão do formato (2). Documentos v1 (`clips`/`subtitles`) são convertidos ao abrir.
    #[serde(default = "v2")]
    pub version: u32,
    /// Trilhas: as de vídeo na ordem V1 (embaixo) .. Vn (em cima); áudio A1..An; legendas S1..Sn.
    #[serde(default)]
    pub tracks: Vec<EditTrack>,
    #[serde(default)]
    pub items: Vec<EditItem>,
    /// Créditos finais (uma linha por item), rolando sobre preto por `credits_seconds` depois do último item.
    #[serde(default)]
    pub credits: Vec<String>,
    #[serde(default = "d_credits_s")]
    pub credits_seconds: f32,
    /// Fade para o preto no fim do programa (quadros).
    #[serde(default)]
    pub fade_out_end: u32,
    /// Marcadores da linha do tempo, em ordem de quadro.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub markers: Vec<EditMarker>,
    /// v1 (convertido ao gravar): sequência de cortes [{cut, in_frame, out_frame?, transition_in?}].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clips: Vec<EditClip>,
    /// v1 (convertido ao gravar): legendas em quadros globais [{start_frame, end_frame, text}].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subtitles: Vec<SubtitleEntry>,
}

fn v2() -> u32 {
    EDIT_VERSION
}
fn d_credits_s() -> f32 {
    6.0
}

impl Default for EditDoc {
    fn default() -> Self {
        EditDoc { version: EDIT_VERSION, tracks: Vec::new(), items: Vec::new(), credits: Vec::new(), credits_seconds: 6.0, fade_out_end: 0, markers: Vec::new(), clips: Vec::new(), subtitles: Vec::new() }
    }
}

/// Fim do programa, início dos créditos e total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Timeline {
    pub credits_start: u32,
    pub total: u32,
}

impl EditDoc {
    /// Marcador no quadro: tira se já há um ali, senão põe (nome "M<n>"). Devolve se ficou um marcador.
    pub fn toggle_marker(&mut self, frame: u32) -> bool {
        if let Some(i) = self.markers.iter().position(|m| m.frame == frame) {
            self.markers.remove(i);
            return false;
        }
        let n = self.markers.len() + 1;
        self.markers.push(EditMarker { frame, name: format!("M{n}") });
        self.markers.sort_by_key(|m| m.frame);
        true
    }

    /// O marcador seguinte (depois de `frame`) ou o anterior.
    pub fn marker_near(&self, frame: u32, forward: bool) -> Option<u32> {
        if forward { self.markers.iter().map(|m| m.frame).find(|f| *f > frame) } else { self.markers.iter().rev().map(|m| m.frame).find(|f| *f < frame) }
    }

    /// Documento vazio com as trilhas padrão: V1, V2, A1, A2, A3, S1.
    pub fn with_default_tracks() -> EditDoc {
        let mut d = EditDoc::default();
        d.ensure_default_tracks();
        d
    }

    fn ensure_default_tracks(&mut self) {
        for (kind, n) in [(TrackKind::Video, 2), (TrackKind::Audio, 3), (TrackKind::Subtitle, 1)] {
            while self.tracks.iter().filter(|t| t.kind == kind).count() < n {
                self.add_track(kind);
            }
        }
    }

    /// Acrescenta uma trilha do tipo e devolve o id.
    pub fn add_track(&mut self, kind: TrackKind) -> u32 {
        let id = self.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        // a trilha nova vai depois da última do mesmo tipo (vídeo, áudio, legenda agrupados nessa ordem)
        let order = |k: TrackKind| match k {
            TrackKind::Video => 0,
            TrackKind::Audio => 1,
            TrackKind::Subtitle => 2,
        };
        let at = self.tracks.iter().rposition(|t| order(t.kind) <= order(kind)).map(|i| i + 1).unwrap_or(0);
        self.tracks.insert(at, EditTrack::new(id, kind));
        id
    }

    pub fn track(&self, id: u32) -> Option<&EditTrack> {
        self.tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: u32) -> Option<&mut EditTrack> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    /// Trilhas de um tipo na ordem (V1, V2... / A1, A2... / S1...).
    pub fn tracks_of(&self, kind: TrackKind) -> Vec<&EditTrack> {
        self.tracks.iter().filter(|t| t.kind == kind).collect()
    }

    /// Nome mostrado da trilha: o próprio ou V/A/S + posição.
    pub fn track_label(&self, id: u32) -> String {
        let Some(t) = self.track(id) else { return "?".into() };
        if !t.name.is_empty() {
            return t.name.clone();
        }
        let n = self.tracks.iter().filter(|x| x.kind == t.kind).position(|x| x.id == id).unwrap_or(0) + 1;
        let p = match t.kind {
            TrackKind::Video => "V",
            TrackKind::Audio => "A",
            TrackKind::Subtitle => "S",
        };
        format!("{p}{n}")
    }

    pub fn item(&self, id: u32) -> Option<&EditItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn item_mut(&mut self, id: u32) -> Option<&mut EditItem> {
        self.items.iter_mut().find(|i| i.id == id)
    }

    pub fn next_item_id(&self) -> u32 {
        self.items.iter().map(|i| i.id).max().unwrap_or(0) + 1
    }

    pub fn next_link_id(&self) -> u32 {
        self.items.iter().filter_map(|i| i.link).max().unwrap_or(0) + 1
    }

    /// Itens de uma trilha, ordenados pelo início.
    pub fn items_on(&self, track: u32) -> Vec<&EditItem> {
        let mut v: Vec<&EditItem> = self.items.iter().filter(|i| i.track == track).collect();
        v.sort_by_key(|i| i.start);
        v
    }

    /// O item e os vinculados a ele.
    pub fn linked(&self, id: u32) -> Vec<u32> {
        match self.item(id).and_then(|i| i.link) {
            Some(l) => self.items.iter().filter(|i| i.link == Some(l)).map(|i| i.id).collect(),
            None => vec![id],
        }
    }

    /// Vizinho que termina exatamente onde `id` começa (mesma trilha).
    pub fn neighbor_before(&self, id: u32) -> Option<&EditItem> {
        let it = self.item(id)?;
        self.items.iter().find(|o| o.track == it.track && o.id != id && o.end() == it.start)
    }

    pub fn neighbor_after(&self, id: u32) -> Option<&EditItem> {
        let it = self.item(id)?;
        self.items.iter().find(|o| o.track == it.track && o.id != id && o.start == it.end())
    }

    /// Fim do programa: o último item de vídeo ou áudio (legendas não alongam a edição).
    pub fn content_end(&self) -> u32 {
        self.items.iter().filter(|i| i.source.track_kind() != TrackKind::Subtitle).map(|i| i.end()).max().unwrap_or(0)
    }

    pub fn timeline(&self, _p: &Project, fps: f32) -> ToolResult<Timeline> {
        let end = self.content_end();
        let credits = if self.credits.is_empty() { 0 } else { (self.credits_seconds * fps).round() as u32 };
        Ok(Timeline { credits_start: end, total: end + credits })
    }

    /// Converte o v1 (`clips`/`subtitles`) para trilhas, garante as trilhas padrão e acerta a versão. Idempotente.
    ///
    /// A conversão desenha e soa igual ao v1: cada clipe vira um item em V1 e o seu som um item vinculado na primeira
    /// trilha de áudio livre (o som de um clipe que entra com dissolve continua sobreposto ao anterior, como no v1);
    /// o dissolve/cortina do v1 (o clipe novo começava antes do fim do anterior) vira transição alinhada ao FIM do
    /// corte usando a alça do clipe novo; os fades do v1 continuam fades na entrada do clipe. Caso degenerado: uma
    /// sobreposição do tamanho do clipe inteiro fica com um quadro a menos.
    pub fn normalize(&mut self, p: &Project) -> ToolResult<()> {
        if !self.clips.is_empty() || !self.subtitles.is_empty() {
            if !self.items.is_empty() {
                return Err(ToolError::Invalid("edição com `clips`/`subtitles` (v1) e `items` (v2) juntos: use um formato só".into()));
            }
            let fps = p.settings.fps() as f32;
            let clips = std::mem::take(&mut self.clips);
            let subs = std::mem::take(&mut self.subtitles);
            let tl = timeline_v1(p, &clips, 0, 0.0, fps)?;
            self.ensure_default_tracks();
            let v1 = self.tracks_of(TrackKind::Video)[0].id;
            let s1 = self.tracks_of(TrackKind::Subtitle)[0].id;
            let mut prev_end = 0u32;
            for (k, c) in clips.iter().enumerate() {
                let (start, dur, off) = tl.items[k];
                let mut ov = prev_end.saturating_sub(start);
                if ov >= dur {
                    ov = dur - 1;
                }
                let link = k as u32 + 1;
                let mut v = EditItem::new(self.next_item_id(), v1, start + ov, dur - ov, ItemSource::Cut { cut: c.cut.clone() });
                v.src_in = off + ov;
                v.link = Some(link);
                v.transition_in = match &c.transition_in {
                    Some(t) if t.kind == TransitionKind::Cut || t.frames == 0 => None,
                    Some(t) if overlaps(t.kind) => (ov > 0).then(|| Transition { kind: t.kind, frames: ov, align: TransitionAlign::End }),
                    Some(t) if k == 0 && !t.kind.is_fade_in() => None,
                    Some(t) => Some(Transition { kind: t.kind, frames: t.frames, align: TransitionAlign::Start }),
                    None => None,
                };
                self.items.push(v);
                // som: na primeira trilha de áudio sem nada no trecho (sobreposição do v1 preservada)
                let lane = self.free_audio_track(start, start + dur);
                let mut a = EditItem::new(self.next_item_id(), lane, start, dur, ItemSource::CutAudio { cut: c.cut.clone(), track: None });
                a.src_in = off;
                a.link = Some(link);
                self.items.push(a);
                prev_end = start + dur;
            }
            for s in subs.into_iter().filter(|s| s.end_frame > s.start_frame) {
                let id = self.next_item_id();
                self.items.push(EditItem::new(id, s1, s.start_frame, s.end_frame - s.start_frame, ItemSource::Caption { text: s.text, speaker: None, style: None }));
            }
        }
        if self.tracks.is_empty() {
            self.ensure_default_tracks();
        }
        self.version = EDIT_VERSION;
        Ok(())
    }

    /// Primeira trilha de áudio sem item em [s, e); cria uma nova se todas estiverem ocupadas.
    pub fn free_audio_track(&mut self, s: u32, e: u32) -> u32 {
        let ids: Vec<u32> = self.tracks_of(TrackKind::Audio).iter().map(|t| t.id).collect();
        for id in ids {
            if !self.items.iter().any(|i| i.track == id && i.start < e && s < i.end()) {
                return id;
            }
        }
        self.add_track(TrackKind::Audio)
    }

    /// Confere trilhas, fontes e sobreposições (mensagens para o usuário).
    pub fn validate(&self, p: &Project) -> ToolResult<()> {
        let bad = |m: String| Err(ToolError::Invalid(m));
        if self.version != EDIT_VERSION {
            return bad(format!("versão da edição {} (esperado {EDIT_VERSION})", self.version));
        }
        let mut seen = std::collections::HashSet::new();
        for t in &self.tracks {
            if !seen.insert(t.id) {
                return bad(format!("trilha {} repetida", t.id));
            }
            if !(t.volume.is_finite() && (0.0..=4.0).contains(&t.volume)) {
                return bad(format!("{}: volume da trilha fora de 0..4", self.track_label(t.id)));
            }
            if let Some(s) = &t.style {
                s.validate().map_err(|e| ToolError::Invalid(format!("{}: {e}", self.track_label(t.id))))?;
            }
        }
        if !(self.credits_seconds.is_finite() && self.credits_seconds >= 0.0 && self.credits_seconds <= 600.0) {
            return bad("credits_seconds fora de 0..600".into());
        }
        let mut ids = std::collections::HashSet::new();
        for it in &self.items {
            let label = format!("item {}", it.id);
            if !ids.insert(it.id) {
                return bad(format!("{label}: id repetido"));
            }
            let Some(t) = self.track(it.track) else { return bad(format!("{label}: trilha {} não existe", it.track)) };
            if t.kind != it.source.track_kind() {
                return bad(format!("{label}: fonte {:?} não cabe na trilha {}", it.source.track_kind(), self.track_label(t.id)));
            }
            if it.dur == 0 {
                return bad(format!("{label}: duração zero"));
            }
            if !(it.speed.is_finite() && (0.1..=10.0).contains(&it.speed)) {
                return bad(format!("{label}: velocidade fora de 0.1..10"));
            }
            if !(it.opacity.is_finite() && (0.0..=1.0).contains(&it.opacity)) || !(it.volume.is_finite() && (0.0..=8.0).contains(&it.volume)) || !(it.pan.is_finite() && (-1.0..=1.0).contains(&it.pan)) {
                return bad(format!("{label}: opacidade (0..1), volume (0..8) ou balanço (-1..1) fora do intervalo"));
            }
            it.keys.validate().map_err(|e| ToolError::Invalid(format!("{label}: {e}")))?;
            it.audio_fx.0.validate().map_err(|e| ToolError::Invalid(format!("{label}: {e}")))?;
            for f in &it.filters {
                f.keys.validate().map_err(|e| ToolError::Invalid(format!("{label}: filtro {}: {e}", f.effect.name())))?;
            }
            match &it.source {
                ItemSource::Cut { cut } | ItemSource::CutAudio { cut, .. } => {
                    let cd = cut_data(p, cut).map_err(|e| ToolError::Invalid(format!("{label}: {e}")))?;
                    if it.src_in >= cd.duration {
                        return bad(format!("{label}: entrada {} além do corte ({} quadros)", it.src_in, cd.duration));
                    }
                }
                ItemSource::Image { asset } => match p.node(asset).map(|n| &n.data) {
                    Some(NodeKind::Asset(a)) if matches!(a.asset, AssetType::Texture | AssetType::Background) && a.blob.is_some() => {}
                    _ => return bad(format!("{label}: {asset} não é imagem (asset texture/background com PNG)")),
                },
                ItemSource::Audio { asset } => match p.node(asset).map(|n| &n.data) {
                    Some(NodeKind::Asset(a)) if a.asset == AssetType::AudioClip => {}
                    _ => return bad(format!("{label}: {asset} não é clipe de áudio")),
                },
                ItemSource::Color { rgba } => {
                    if rgba.iter().any(|v| !v.is_finite()) {
                        return bad(format!("{label}: cor inválida"));
                    }
                }
                ItemSource::Caption { style, .. } => {
                    if let Some(s) = style {
                        s.validate().map_err(|e| ToolError::Invalid(format!("{label}: {e}")))?;
                    }
                }
            }
            for tr in [&it.transition_in, &it.transition_out].into_iter().flatten() {
                if tr.frames > 24 * 60 {
                    return bad(format!("{label}: transição longa demais"));
                }
            }
        }
        for t in &self.tracks {
            let v = self.items_on(t.id);
            for w in v.windows(2) {
                if w[1].start < w[0].end() {
                    return bad(format!("{}: itens {} e {} se sobrepõem", self.track_label(t.id), w[0].id, w[1].id));
                }
            }
        }
        for it in &self.items {
            if let Some(a) = it.anchor {
                if self.item(a).is_none() {
                    return bad(format!("item {}: âncora {a} não existe", it.id));
                }
            }
        }
        Ok(())
    }

    /// Vista v1 da primeira trilha de vídeo (para exportadores que só entendem uma sequência com sobreposição, como o
    /// OTIO): (corte, (início, duração, deslocamento no corte), transição de entrada). Inverso exato da migração.
    pub fn v1_view(&self) -> Vec<(Id, (u32, u32, u32), Option<Transition>)> {
        let Some(v1) = self.tracks_of(TrackKind::Video).first().map(|t| t.id) else { return Vec::new() };
        let items: Vec<&EditItem> = self.items_on(v1).into_iter().filter(|i| i.enabled).collect();
        let mut out: Vec<(Id, (u32, u32, u32), Option<Transition>)> = Vec::new();
        for (k, it) in items.iter().enumerate() {
            let Some(cut) = (match &it.source {
                ItemSource::Cut { cut } => Some(cut.clone()),
                _ => None,
            }) else {
                continue;
            };
            let mut start = it.start;
            let mut off = it.src_in;
            let mut dur = it.dur;
            let adjacent = k > 0 && items[k - 1].end() == it.start;
            if let Some(tr) = it.transition_in.as_ref().filter(|t| adjacent && !t.kind.is_fade_in() && t.kind != TransitionKind::Cut) {
                // a sobreposição do v1 é o trecho da transição: o novo começa `l` antes do corte (alça de entrada) e o
                // anterior segue `r` depois dele (alça de saída)
                let (l, r) = tr.split();
                let back = l.min(off).min(start);
                start -= back;
                off -= back;
                dur += back;
                if let Some(prev) = out.last_mut() {
                    prev.1.1 += r;
                }
            }
            out.push((cut, (start, dur, off), it.transition_in.clone()));
        }
        out
    }
}
