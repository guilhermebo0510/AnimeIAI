//! Modelo do documento (ADR-002). Todos os objetos editáveis são [`Node`]s num mapa plano por id, com pai e filhos
//! ordenados; o conteúdo específico fica em [`NodeKind`]. Isso dá ao Command Bus operações genéricas (criar, apagar,
//! mover, editar propriedades) que valem para qualquer tipo, e ao MCP uma superfície uniforme.

use crate::blob::BlobHash;
use crate::color::{ColorRef, Rgba};
use crate::ids::Id;
use crate::keys::KeyTracks;
use crate::stroke::Stroke;
use crate::xsheet::XSheet;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------- projeto

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Project {
    pub format: u32,
    pub name: String,
    pub settings: Settings,
    #[serde(default)]
    pub bible: Bible,
    /// Raízes por categoria (ordem = ordem de exibição).
    #[serde(default)]
    pub roots: Roots,
    pub nodes: BTreeMap<Id, Node>,
    /// Revisão do documento; sobe a cada comando aplicado/desfeito/refeito.
    #[serde(default)]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    /// Taxa de quadros como fração (23,976 = 24000/1001).
    pub fps_num: u32,
    pub fps_den: u32,
    /// Idioma principal das falas (ex.: "ja") e idioma da legenda (ex.: "pt-BR").
    #[serde(default = "default_dialogue_lang")]
    pub dialogue_language: String,
    #[serde(default = "default_subtitle_lang")]
    pub subtitle_language: String,
    #[serde(default = "default_sample_rate")]
    pub audio_sample_rate: u32,
    /// Mistura das camadas em luz linear (auditoria 4.6h): as cores viram luz (sRGB -> linear) antes de misturar e
    /// voltam no fim; brilhos, sombras multiplicadas e degradês de luz ficam fisicamente certos (o "Adicionar" estoura
    /// como luz de verdade). Desligado = mistura em sRGB, como o Clip Studio e o Photoshop fazem por padrão.
    #[serde(default)]
    pub linear_light: bool,
    /// Projeto em arquivos por corte (auditoria 4.6k): `project.json` vira o manifesto e cada corte (com camadas e cels)
    /// vai para `cuts/<corte>.json`, regravado só quando muda (merges limpos no git, um artista por corte). Cada corte
    /// editado ganha a trava `cuts/<corte>.lock`; o corte travado por outro processo não aceita edição aqui.
    #[serde(default)]
    pub split_cuts: bool,
}

fn default_dialogue_lang() -> String {
    "ja".into()
}
fn default_subtitle_lang() -> String {
    "pt-BR".into()
}
fn default_sample_rate() -> u32 {
    48_000
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            width: 1920,
            height: 1080,
            fps_num: 24_000,
            fps_den: 1001,
            dialogue_language: default_dialogue_lang(),
            subtitle_language: default_subtitle_lang(),
            audio_sample_rate: default_sample_rate(),
            linear_light: false,
            split_cuts: false,
        }
    }
}

impl Settings {
    pub fn fps(&self) -> f64 {
        self.fps_num as f64 / self.fps_den as f64
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Bible {
    #[serde(default)]
    pub logline: String,
    #[serde(default)]
    pub synopsis: String,
    #[serde(default)]
    pub world: String,
    #[serde(default)]
    pub themes: Vec<String>,
    #[serde(default)]
    pub tone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Roots {
    #[serde(default)]
    pub episodes: Vec<Id>,
    #[serde(default)]
    pub characters: Vec<Id>,
    #[serde(default)]
    pub palettes: Vec<Id>,
    /// Texturas, presets de pincel, LUTs, clipes de áudio, cenários reutilizáveis, efeitos salvos.
    #[serde(default)]
    pub assets: Vec<Id>,
    /// Roteiros, storyboards, edições.
    #[serde(default)]
    pub documents: Vec<Id>,
}

/// Categoria de raiz, usada por comandos que criam nós sem pai.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RootCategory {
    Episodes,
    Characters,
    Palettes,
    Assets,
    Documents,
}

impl Roots {
    pub fn list(&self, c: RootCategory) -> &Vec<Id> {
        match c {
            RootCategory::Episodes => &self.episodes,
            RootCategory::Characters => &self.characters,
            RootCategory::Palettes => &self.palettes,
            RootCategory::Assets => &self.assets,
            RootCategory::Documents => &self.documents,
        }
    }
    pub fn list_mut(&mut self, c: RootCategory) -> &mut Vec<Id> {
        match c {
            RootCategory::Episodes => &mut self.episodes,
            RootCategory::Characters => &mut self.characters,
            RootCategory::Palettes => &mut self.palettes,
            RootCategory::Assets => &mut self.assets,
            RootCategory::Documents => &mut self.documents,
        }
    }
    pub fn all(&self) -> [(RootCategory, &Vec<Id>); 5] {
        [
            (RootCategory::Episodes, &self.episodes),
            (RootCategory::Characters, &self.characters),
            (RootCategory::Palettes, &self.palettes),
            (RootCategory::Assets, &self.assets),
            (RootCategory::Documents, &self.documents),
        ]
    }
}

// ---------------------------------------------------------------- nó genérico

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub id: Id,
    pub name: String,
    /// `None` = nó de raiz (está em `Project.roots`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<Id>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Id>,
    pub data: NodeKind,
    #[serde(default)]
    pub provenance: Provenance,
}

/// Quem criou e quem alterou por último (auditoria; sem IA generativa, `tool:*` marca operações automáticas
/// como intercalação ou preenchimento propagado).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct Provenance {
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub modified_by: String,
    /// Revisão do projeto na última alteração deste nó.
    #[serde(default)]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NodeKind {
    Episode(EpisodeData),
    Scene(SceneData),
    Cut(CutData),
    Layer(LayerData),
    Cel(CelData),
    Character(CharacterData),
    Palette(PaletteData),
    Asset(AssetData),
    Document(DocumentData),
}

impl NodeKind {
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Episode(_) => "episode",
            NodeKind::Scene(_) => "scene",
            NodeKind::Cut(_) => "cut",
            NodeKind::Layer(_) => "layer",
            NodeKind::Cel(_) => "cel",
            NodeKind::Character(_) => "character",
            NodeKind::Palette(_) => "palette",
            NodeKind::Asset(_) => "asset",
            NodeKind::Document(_) => "document",
        }
    }

    /// Tipos de filho aceitos (a hierarquia do ADR-002).
    pub fn accepts_child(&self, child: &NodeKind) -> bool {
        match (self, child) {
            (NodeKind::Episode(_), NodeKind::Scene(_)) => true,
            (NodeKind::Scene(_), NodeKind::Cut(_)) => true,
            (NodeKind::Cut(_), NodeKind::Layer(_)) => true,
            (NodeKind::Layer(l), NodeKind::Layer(_)) => l.kind == LayerType::Group,
            (NodeKind::Layer(l), NodeKind::Cel(_)) => l.kind.holds_cels(),
            // Personagem guarda seus documentos de model sheet/expressões como cortes-canvas e suas partes como camadas.
            (NodeKind::Character(_), NodeKind::Cut(_)) => true,
            (NodeKind::Document(_), NodeKind::Cut(_)) => true,
            (NodeKind::Document(_), NodeKind::Document(_)) => true,
            _ => false,
        }
    }

    /// Categoria de raiz permitida para este tipo (nós sem pai).
    pub fn root_category(&self) -> Option<RootCategory> {
        match self {
            NodeKind::Episode(_) => Some(RootCategory::Episodes),
            NodeKind::Character(_) => Some(RootCategory::Characters),
            NodeKind::Palette(_) => Some(RootCategory::Palettes),
            NodeKind::Asset(_) => Some(RootCategory::Assets),
            NodeKind::Document(_) => Some(RootCategory::Documents),
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        match self {
            NodeKind::Cut(c) => c.validate(),
            NodeKind::Layer(l) => l.validate(),
            NodeKind::Cel(c) => c.validate(),
            NodeKind::Palette(p) => p.validate(),
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------- episódio, cena, corte

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct EpisodeData {
    #[serde(default)]
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct SceneData {
    #[serde(default)]
    pub number: u32,
    #[serde(default)]
    pub description: String,
}

/// Etapas de produção do corte (ordem do pipeline japonês).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Layout,
    Genga,
    Douga,
    Shiage,
    Background,
    Satsuei,
    Color,
    Sound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    #[default]
    NotStarted,
    InProgress,
    Review,
    Approved,
    Retake,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CutData {
    /// Número do corte (カット番号).
    #[serde(default)]
    pub number: u32,
    /// Duração em quadros.
    pub duration: u32,
    /// Tamanho do quadro deste corte; `None` = configuração do projeto. Model sheets e cenários grandes usam outro tamanho.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<[u32; 2]>,
    #[serde(default)]
    pub xsheet: XSheet,
    #[serde(default)]
    pub stages: BTreeMap<Stage, StageStatus>,
    #[serde(default)]
    pub notes: String,
    /// Falas do roteiro vinculadas (ids de elementos do documento de roteiro).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dialogue_links: Vec<Id>,
    /// Painel de storyboard de origem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storyboard_panel: Option<Id>,
    /// Câmera do corte (centro, zoom, rotação, chaves, tremor).
    #[serde(default)]
    pub camera: CameraData,
    /// Clipes de áudio posicionados no corte (falas, SFX, BGM, ambiente).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio: Vec<AudioPlacement>,
    /// Fase da animação do corte (原画 genga: poses-chave; 動画 douga: limpeza e intercalações). Muda o layout do
    /// workspace Animação, o padrão da mesa de luz e trava os cels-chave contra edição acidental no douga.
    #[serde(default, skip_serializing_if = "CutPhase::is_genga")]
    pub phase: CutPhase,
    /// Gestão (workspace Produção), tudo opcional: prazo do corte em `AAAA-MM-DD`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    /// Gestão: responsável pelo corte (nome livre).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    /// Gestão: peso/dificuldade do corte (esforço estimado, 1 = corte médio).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    /// Iluminação do corte (auditoria 4.7e): a variante das paletas usada neste corte ("entardecer", "noite"...). `None`
    /// = a variante ativa de cada paleta. Cores sem essa variante ficam com a normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette_variant: Option<String>,
}

/// Fase da animação de um corte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CutPhase {
    #[default]
    Genga,
    Douga,
}

impl CutPhase {
    pub fn is_genga(&self) -> bool {
        *self == CutPhase::Genga
    }
}

/// Trilha de áudio (mixer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum AudioTrack {
    #[default]
    Dialogue,
    Sfx,
    Bgm,
    Ambience,
}

/// Um clipe de áudio (asset `audio_clip`) colocado no tempo do corte.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AudioPlacement {
    #[serde(default = "empty_id")]
    pub id: Id,
    /// Asset de áudio (WAV em blob).
    pub asset: Id,
    #[serde(default)]
    pub track: AudioTrack,
    /// Quadro (0-based) onde o clipe começa.
    pub start_frame: u32,
    /// Corte do início do clipe, em segundos.
    #[serde(default)]
    pub trim_start: f32,
    /// Duração usada (s); `None` = até o fim do clipe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(default)]
    pub gain_db: f32,
    /// -1 (esquerda) .. 1 (direita).
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub fade_in: f32,
    #[serde(default)]
    pub fade_out: f32,
    /// Personagem que fala (falas) — liga ao lip-sync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub character: Option<Id>,
}

impl CutData {
    pub fn new(duration: u32) -> Self {
        CutData {
            number: 0,
            duration,
            canvas: None,
            xsheet: XSheet::default(),
            stages: BTreeMap::new(),
            notes: String::new(),
            dialogue_links: Vec::new(),
            storyboard_panel: None,
            camera: CameraData::default(),
            audio: Vec::new(),
            phase: CutPhase::Genga,
            due: None,
            assignee: None,
            weight: None,
            palette_variant: None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.duration == 0 || self.duration > 24 * 60 * 30 {
            return Err(format!("duração do corte fora do intervalo: {}", self.duration));
        }
        if let Some(w) = self.weight {
            if !(w.is_finite() && w > 0.0 && w <= 1000.0) {
                return Err(format!("peso do corte fora do intervalo (0..1000]: {w}"));
            }
        }
        if let Some(d) = &self.due {
            let b = d.as_bytes();
            let shape = b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
            if !shape {
                return Err(format!("prazo do corte deve ser AAAA-MM-DD: {d}"));
            }
        }
        if let Some([w, h]) = self.canvas {
            if w == 0 || h == 0 || w > 16384 || h > 16384 {
                return Err(format!("canvas inválido {w}x{h}"));
            }
        }
        for a in &self.audio {
            if !(a.gain_db.is_finite() && (-1.0..=1.0).contains(&a.pan) && a.fade_in >= 0.0 && a.fade_out >= 0.0 && a.trim_start >= 0.0) {
                return Err(format!("clipe de áudio {} com parâmetros inválidos", a.id));
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for t in &self.xsheet.tracks {
            if !names.insert(&t.id) {
                return Err(format!("trilha duplicada {}", t.id));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraData {
    /// Centro do quadro de câmera em pixels do canvas; `None` = centro do canvas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub center: Option<[f32; 2]>,
    /// 1 = quadro do tamanho do projeto.
    pub zoom: f32,
    /// Graus.
    pub rotation: f32,
    /// Keyframes: center_x, center_y, zoom, rotation, shake (amplitude em px).
    #[serde(default, skip_serializing_if = "KeyTracks::is_empty")]
    pub keys: KeyTracks,
    /// Semente do tremor de câmera.
    #[serde(default)]
    pub shake_seed: u64,
}

impl Default for CameraData {
    fn default() -> Self {
        CameraData { center: None, zoom: 1.0, rotation: 0.0, keys: KeyTracks::default(), shake_seed: 0 }
    }
}

// ---------------------------------------------------------------- camadas e cels

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayerType {
    Group,
    #[default]
    Vector,
    Raster,
    Fill,
    Text,
    Guide,
    Reference,
    Adjustment,
    Effect,
    Particles,
    /// Modelo 3D de referência (S24): GLB do ativo renderizado por quadro com a câmera e a transformação de
    /// `params.model3d` (ver [`crate::model3d`]). Sem cels.
    Model3d,
}

impl LayerType {
    /// Camadas cujo conteúdo é por cel (desenho que pode mudar quadro a quadro).
    pub fn holds_cels(&self) -> bool {
        matches!(
            self,
            LayerType::Vector | LayerType::Raster | LayerType::Fill | LayerType::Text | LayerType::Guide | LayerType::Reference
        )
    }
    /// Nunca saem no render final.
    pub fn is_non_rendering(&self) -> bool {
        matches!(self, LayerType::Guide | LayerType::Reference)
    }
}

/// Modo da camada-máscara (`LayerData::mask`): alfa, luma (× alfa) ou o inverso de cada um.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaskMode {
    Alpha,
    AlphaInverted,
    #[default]
    Luma,
    LumaInverted,
}

impl MaskMode {
    fn is_default(&self) -> bool {
        *self == MaskMode::Luma
    }

    /// O valor da máscara num pixel pré-multiplicado `[r, g, b, a]`.
    pub fn value(self, p: [f32; 4]) -> f32 {
        let a = p[3].clamp(0.0, 1.0);
        let luma = if a <= 0.0 { 0.0 } else { (0.2126 * p[0] / a + 0.7152 * p[1] / a + 0.0722 * p[2] / a).clamp(0.0, 1.0) * a };
        match self {
            MaskMode::Alpha => a,
            MaskMode::AlphaInverted => 1.0 - a,
            MaskMode::Luma => luma,
            MaskMode::LumaInverted => 1.0 - luma,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    /// Só para pastas: os filhos se misturam direto com o que está abaixo da pasta.
    PassThrough,
    Multiply,
    Screen,
    Overlay,
    SoftLight,
    HardLight,
    Add,
    Subtract,
    Darken,
    Lighten,
    Difference,
    Color,
    Hue,
    Saturation,
    Luminosity,
    /// Só pinta onde já há pixels abaixo na mesma pasta é tratado por `clip_to_below`; este modo pinta ATRÁS.
    Behind,
}

/// Papel da camada no pipeline (para filtros, travas por etapa e organização).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayerRole {
    #[default]
    None,
    Layout,
    Genga,
    Douga,
    Shiage,
    Shadow,
    Highlight,
    Background,
    Book,
    Effect,
    Correction,
    Construction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LayerData {
    pub kind: LayerType,
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub locked: bool,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    /// Máscara de recorte: só aparece onde a camada imediatamente abaixo tem alfa.
    #[serde(default)]
    pub clip_to_below: bool,
    /// Camada usada como máscara (raster em escala de cinza ou vetor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<Id>,
    /// O que da camada-máscara vale (como os track mattes do After Effects). Padrão: luma × alfa (o de sempre).
    #[serde(default, skip_serializing_if = "MaskMode::is_default")]
    pub mask_mode: MaskMode,
    #[serde(default)]
    pub role: LayerRole,
    /// Profundidade para multiplano (0 = plano da câmera; maior = mais longe).
    #[serde(default)]
    pub depth: f32,
    /// Deslocamento da camada em pixels (cenário maior que o quadro, pan).
    #[serde(default)]
    pub offset: [f32; 2],
    /// Camada de preenchimento: camada(s) de linha usadas como fronteira do balde.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub line_refs: Vec<Id>,
    /// Parâmetros específicos (efeito, ajuste, partículas, texto, referência) — esquema por tipo, definido nas sprints.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub params: serde_json::Value,
    /// Keyframes: offset_x, offset_y, opacity, rotation (graus, horário), scale, anchor_x, anchor_y.
    #[serde(default, skip_serializing_if = "KeyTracks::is_empty")]
    pub keys: KeyTracks,
    /// Escala fixa da camada (sem chave de `scale`; 1 = tamanho do canvas). Composição (S24 3.2).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f32,
    /// Rotação fixa em graus, horário (sem chave de `rotation`).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f32,
    /// Ponto de âncora (pivô da escala e da rotação) em px a partir do centro do canvas; [0, 0] = centro. Mover só a
    /// âncora não mexe na camada parada (a ferramenta Âncora compensa o deslocamento quando há escala/rotação).
    #[serde(default, skip_serializing_if = "is_zero2")]
    pub anchor: [f32; 2],
    /// Cor de rótulo na Linha do tempo da composição (0 = nenhuma; 1..=8 = paleta de rótulos da interface).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub label: u8,
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
fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}
fn is_zero2(v: &[f32; 2]) -> bool {
    v[0] == 0.0 && v[1] == 0.0
}

impl LayerData {
    pub fn new(kind: LayerType) -> Self {
        LayerData {
            kind,
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: if kind == LayerType::Group { BlendMode::PassThrough } else { BlendMode::Normal },
            clip_to_below: false,
            mask: None,
            mask_mode: MaskMode::Luma,
            role: LayerRole::None,
            depth: 0.0,
            offset: [0.0, 0.0],
            line_refs: Vec::new(),
            params: serde_json::Value::Null,
            keys: KeyTracks::default(),
            scale: 1.0,
            rotation: 0.0,
            anchor: [0.0, 0.0],
            label: 0,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(format!("opacidade fora de 0..1: {}", self.opacity));
        }
        if self.blend == BlendMode::PassThrough && self.kind != LayerType::Group {
            return Err("'pass_through' só vale para pastas".into());
        }
        if !(self.depth.is_finite() && self.offset.iter().all(|v| v.is_finite())) {
            return Err("profundidade/deslocamento não finitos".into());
        }
        if !(self.scale.is_finite() && self.rotation.is_finite() && self.anchor.iter().all(|v| v.is_finite())) {
            return Err("escala/rotação/âncora não finitas".into());
        }
        if self.kind == LayerType::Model3d {
            match crate::model3d::Model3dParams::from_layer_params(&self.params) {
                Some(r) => r?,
                None => return Err("camada 3D sem params.model3d".into()),
            };
            crate::model3d::validate_keys(&self.keys)?;
        }
        self.keys.validate()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CelRole {
    #[default]
    Key,
    Breakdown,
    Inbetween,
}

/// Coordenada de tile raster (tiles de [`TILE_SIZE`] px; podem ser negativos — cenário além do quadro).
pub const TILE_SIZE: u32 = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TileEntry {
    pub x: i32,
    pub y: i32,
    pub blob: BlobHash,
}

/// Região preenchida pelo balde (detalhada na S07): semente + índice de cor; a máscara rasterizada fica em blob.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FillRegion {
    #[serde(default = "empty_id")]
    pub id: Id,
    pub seed: [f32; 2],
    pub color: ColorRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<BlobHash>,
    /// Retângulo da máscara em pixels do canvas [x, y, w, h].
    #[serde(default)]
    pub bounds: [i32; 4],
    /// Degradê no lugar da cor chapada (mapeado na caixa `bounds` da região); `None` = `color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gradient: Option<crate::paint::Gradient>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CelData {
    /// Número do cel dentro da camada (o "3" de "A3"); único por camada.
    pub number: u32,
    #[serde(default)]
    pub role: CelRole,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Stroke>,
    /// Tiles raster (ordenados por y, x).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiles: Vec<TileEntry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<FillRegion>,
    /// Texto (camadas de texto/balão).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Imagem importada (camadas de referência) — blob PNG.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<BlobHash>,
    /// Transformação da imagem/cel: [tx, ty, escala, rotação em graus].
    #[serde(default = "identity_xform")]
    pub transform: [f32; 4],
    /// De onde veio este desenho quando é uma intercalação aceita pela prévia (R16 REQ-P04). Ausente = sem registro
    /// (`UnknownLegacy`): desenhos antigos, feitos à mão ou pelo intercalador em lote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inbetween_origin: Option<InbetweenOrigin>,
}

/// Registro persistente da intercalação aceita (R16 REQ-P04): vai junto com o conteúdo do cel (desfazer/refazer,
/// cópias). Não prova quem desenhou: `source` é o caminho da entrada do aceite.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InbetweenOrigin {
    /// Fonte da entrada do aceite.
    pub source: crate::session::InputSource,
    /// Mecanismo que gerou os traços ("inbetween" = `aiai_anim::inbetween`).
    pub mechanism: String,
    /// Versão do motor: commit-base do código que gerou o candidato.
    pub engine: String,
    /// Opções do motor usadas (`InbetweenOptions`, já com os pares manuais convertidos).
    pub options: serde_json::Value,
    pub key_a: u32,
    pub key_b: u32,
    pub cel_a: Id,
    pub cel_b: Id,
    /// Traços fonte de A e de B (ids, na ordem dos cels).
    pub strokes_a: Vec<Id>,
    pub strokes_b: Vec<Id>,
    pub frame_a: u32,
    pub frame_b: u32,
    pub frame: u32,
    pub t: f32,
    /// Revisão do projeto em que a prévia foi feita.
    pub revision: u64,
}

fn empty_id() -> Id {
    Id(String::new())
}

fn identity_xform() -> [f32; 4] {
    [0.0, 0.0, 1.0, 0.0]
}

impl CelData {
    pub fn new(number: u32) -> Self {
        CelData {
            number,
            role: CelRole::Key,
            strokes: Vec::new(),
            tiles: Vec::new(),
            fills: Vec::new(),
            text: String::new(),
            image: None,
            transform: identity_xform(),
            inbetween_origin: None,
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.number == 0 {
            return Err("número de cel começa em 1".into());
        }
        let mut ids = std::collections::BTreeSet::new();
        for s in &self.strokes {
            s.validate().map_err(|e| format!("traço {}: {e}", s.id))?;
            if !ids.insert(&s.id) {
                return Err(format!("traço duplicado {}", s.id));
            }
        }
        let mut tiles = std::collections::BTreeSet::new();
        for t in &self.tiles {
            if !tiles.insert((t.y, t.x)) {
                return Err(format!("tile duplicado ({}, {})", t.x, t.y));
            }
        }
        if !self.transform.iter().all(|v| v.is_finite()) || self.transform[2] <= 0.0 {
            return Err("transformação do cel inválida".into());
        }
        Ok(())
    }

    pub fn tile(&self, x: i32, y: i32) -> Option<&BlobHash> {
        self.tiles.iter().find(|t| t.x == x && t.y == y).map(|t| &t.blob)
    }

    /// Mantém `tiles` ordenado por (y, x) — forma canônica.
    pub fn sort_tiles(&mut self) {
        self.tiles.sort_by_key(|t| (t.y, t.x));
    }
}

// ---------------------------------------------------------------- personagens, paletas, assets, documentos

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct CharacterData {
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub age: String,
    /// Altura em cabeças (proporção do model sheet).
    #[serde(default = "default_heads")]
    pub heads_tall: f32,
    /// Paleta de settei deste personagem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Id>,
    /// Perfil de voz (motor, voz, ajustes) — esquema definido na S17.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub voice: serde_json::Value,
    /// Medidas de referência do model sheet para o medidor de consistência (S10).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub measurements: serde_json::Value,
    /// Pasta do personagem: os materiais (folha de modelo, expressões, boca, olhos, figurinos, extras), na ordem de
    /// exibição. Projetos antigos não têm a lista; os cortes soltos do personagem entram como materiais ao ler
    /// (`aiai_tools::char_material::materials`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub materials: Vec<CharMaterial>,
}

fn default_heads() -> f32 {
    6.5
}

/// Tipo de material da pasta do personagem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    /// Folha de modelo (turnaround: frente, 3/4, perfil, costas).
    ModelSheet,
    /// Folha de expressões do rosto.
    Expressions,
    /// Tabela de bocas do lip-sync.
    MouthChart,
    /// Detalhe dos olhos.
    Eyes,
    /// Figurino (folha própria + variante de paleta).
    Outfit,
    /// Folha extra livre.
    #[default]
    Other,
}

impl MaterialKind {
    pub const ALL: [MaterialKind; 6] = [MaterialKind::ModelSheet, MaterialKind::Expressions, MaterialKind::MouthChart, MaterialKind::Eyes, MaterialKind::Outfit, MaterialKind::Other];

    /// Chave estável (`model_sheet`, `expressions`...), a mesma do serde.
    pub fn key(self) -> &'static str {
        match self {
            MaterialKind::ModelSheet => "model_sheet",
            MaterialKind::Expressions => "expressions",
            MaterialKind::MouthChart => "mouth_chart",
            MaterialKind::Eyes => "eyes",
            MaterialKind::Outfit => "outfit",
            MaterialKind::Other => "other",
        }
    }

    pub fn from_key(key: &str) -> Option<MaterialKind> {
        Self::ALL.into_iter().find(|k| k.key() == key)
    }
}

/// Conteúdo de um material: sempre um corte-canvas filho do personagem (a folha desenhável, com camadas); a imagem
/// importada também vive num corte (camada de referência com a imagem + camada de linha por cima para anotar) e guarda o
/// blob original.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MaterialContent {
    /// Folha desenhável.
    Sheet { cut: Id },
    /// Imagem importada (PNG em blob) mostrada no corte `cut`.
    Image { blob: BlobHash, cut: Id },
}

impl MaterialContent {
    /// Corte que mostra o material.
    pub fn cut(&self) -> &Id {
        match self {
            MaterialContent::Sheet { cut } | MaterialContent::Image { cut, .. } => cut,
        }
    }
}

/// Um material da pasta do personagem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CharMaterial {
    pub id: Id,
    #[serde(default)]
    pub kind: MaterialKind,
    pub name: String,
    pub content: MaterialContent,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// Figurino: a variante de paleta (settei) deste material.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Id>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NamedColor {
    #[serde(default = "empty_id")]
    pub id: Id,
    pub name: String,
    /// Agrupamento no settei ("pele", "cabelo"...).
    #[serde(default)]
    pub group: String,
    /// Papel: "normal", "sombra", "sombra2", "realce", "linha"...
    #[serde(default)]
    pub role: String,
    pub rgba: Rgba,
    /// Variantes de iluminação ("entardecer", "noite") → cor.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variants: BTreeMap<String, Rgba>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct PaletteData {
    #[serde(default)]
    pub colors: Vec<NamedColor>,
    /// Variante ativa ("" = normal).
    #[serde(default)]
    pub active_variant: String,
    /// Degradês guardados na paleta (vão para a biblioteca com ela).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gradients: Vec<crate::paint::NamedGradient>,
}

impl PaletteData {
    pub fn color(&self, id: &Id) -> Option<&NamedColor> {
        self.colors.iter().find(|c| &c.id == id)
    }

    /// Cor efetiva considerando a variante ativa.
    pub fn resolve(&self, id: &Id) -> Option<Rgba> {
        self.resolve_in(id, None)
    }

    /// Cor efetiva na variante `variant` (a de um corte); `None` = a variante ativa da paleta. Sem a variante, a normal.
    pub fn resolve_in(&self, id: &Id, variant: Option<&str>) -> Option<Rgba> {
        let c = self.color(id)?;
        let v = variant.unwrap_or(&self.active_variant);
        Some(c.variants.get(v).copied().unwrap_or(c.rgba))
    }

    /// Nomes das variantes que alguma cor tem (sem a normal), em ordem.
    pub fn variant_names(&self) -> Vec<String> {
        let mut v: std::collections::BTreeSet<String> = self.colors.iter().flat_map(|c| c.variants.keys().cloned()).collect();
        if !self.active_variant.is_empty() {
            v.insert(self.active_variant.clone());
        }
        v.into_iter().collect()
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::BTreeSet::new();
        for c in &self.colors {
            if !ids.insert(&c.id) {
                return Err(format!("cor duplicada {}", c.id));
            }
            if !c.rgba.is_valid() || !c.variants.values().all(|v| v.is_valid()) {
                return Err(format!("cor fora de 0..1 em {}", c.name));
            }
        }
        for g in &self.gradients {
            g.gradient.validate().map_err(|e| format!("degradê '{}': {e}", g.name))?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Texture,
    BrushPreset,
    Lut,
    AudioClip,
    Background,
    EffectPreset,
    Font,
    /// Modelo 3D (S24): blob = GLB normalizado pelo importador; `params.model3d` = informações (triângulos, caixa,
    /// licença, arquivo de origem).
    Model3d,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssetData {
    pub asset: AssetType,
    /// Conteúdo binário principal (PNG, WAV, .cube...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob: Option<BlobHash>,
    /// Parâmetros (gerador procedural, preset de pincel, texto/ajustes de origem de uma fala TTS...).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub params: serde_json::Value,
    /// Origem e licença (ex.: "procedural", "desenhado no app", "CC0: <url>", "VOICEVOX: <personagem>").
    #[serde(default)]
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    Script,
    Storyboard,
    Edit,
    Notes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DocumentData {
    pub doc: DocumentType,
    /// Conteúdo estruturado (elementos do roteiro, painéis, timeline de edição) — esquema definido nas S16/S18.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    #[schemars(with = "Option<serde_json::Value>")]
    pub content: serde_json::Value,
}

/// Blobs citados dentro de JSON livre (documentos, parâmetros de efeito/pincel, retakes): qualquer string com cara de
/// hash BLAKE3 (64 hex) conta como referência — melhor guardar um blob a mais do que perder um.
fn scan_blob_hashes(v: &serde_json::Value, out: &mut std::collections::BTreeSet<BlobHash>) {
    match v {
        serde_json::Value::String(s) => {
            let h = BlobHash(s.clone());
            if h.is_well_formed() {
                out.insert(h);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| scan_blob_hashes(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| scan_blob_hashes(x, out)),
        _ => {}
    }
}

// ---------------------------------------------------------------- projeto: helpers

impl Project {
    pub fn new(name: &str) -> Self {
        Project {
            format: crate::FORMAT_VERSION,
            name: name.to_string(),
            settings: Settings::default(),
            bible: Bible::default(),
            roots: Roots::default(),
            nodes: BTreeMap::new(),
            revision: 0,
        }
    }

    pub fn node(&self, id: &Id) -> Option<&Node> {
        self.nodes.get(id)
    }

    pub fn node_mut(&mut self, id: &Id) -> Option<&mut Node> {
        self.nodes.get_mut(id)
    }

    /// Todos os ids da subárvore de `id` (inclusive), em pré-ordem.
    pub fn subtree(&self, id: &Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut stack = vec![id.clone()];
        while let Some(cur) = stack.pop() {
            if let Some(n) = self.nodes.get(&cur) {
                out.push(cur.clone());
                for c in n.children.iter().rev() {
                    stack.push(c.clone());
                }
            }
        }
        out
    }

    /// Ancestrais de `id`, do pai até a raiz.
    pub fn ancestors(&self, id: &Id) -> Vec<Id> {
        let mut out = Vec::new();
        let mut cur = self.nodes.get(id).and_then(|n| n.parent.clone());
        while let Some(p) = cur {
            cur = self.nodes.get(&p).and_then(|n| n.parent.clone());
            out.push(p);
        }
        out
    }

    /// Corte que contém `id` (o próprio, se for corte).
    pub fn cut_of(&self, id: &Id) -> Option<Id> {
        std::iter::once(id.clone())
            .chain(self.ancestors(id))
            .find(|n| matches!(self.nodes.get(n).map(|n| &n.data), Some(NodeKind::Cut(_))))
    }

    /// Cel de número `number` da camada `layer`.
    pub fn cel_by_number(&self, layer: &Id, number: u32) -> Option<&Node> {
        let l = self.nodes.get(layer)?;
        l.children.iter().filter_map(|c| self.nodes.get(c)).find(|n| matches!(&n.data, NodeKind::Cel(c) if c.number == number))
    }

    /// Blobs obrigatórios: tiles, máscaras, imagens de cel e conteúdo de assets (faltar um é erro).
    pub fn required_blobs(&self) -> std::collections::BTreeSet<BlobHash> {
        let mut out = std::collections::BTreeSet::new();
        for n in self.nodes.values() {
            match &n.data {
                NodeKind::Cel(c) => {
                    out.extend(c.tiles.iter().map(|t| t.blob.clone()));
                    out.extend(c.fills.iter().filter_map(|f| f.mask.clone()));
                    out.extend(c.image.clone());
                }
                NodeKind::Asset(a) => out.extend(a.blob.clone()),
                _ => {}
            }
        }
        out
    }

    /// Todos os hashes de blob referenciados (obrigatórios + citados em JSON livre) — persistência e coleta de lixo.
    pub fn referenced_blobs(&self) -> std::collections::BTreeSet<BlobHash> {
        let mut out = std::collections::BTreeSet::new();
        for n in self.nodes.values() {
            match &n.data {
                NodeKind::Cel(c) => {
                    out.extend(c.tiles.iter().map(|t| t.blob.clone()));
                    out.extend(c.fills.iter().filter_map(|f| f.mask.clone()));
                    out.extend(c.image.clone());
                }
                NodeKind::Asset(a) => {
                    out.extend(a.blob.clone());
                    scan_blob_hashes(&a.params, &mut out);
                }
                NodeKind::Document(d) => scan_blob_hashes(&d.content, &mut out),
                NodeKind::Layer(l) => scan_blob_hashes(&l.params, &mut out),
                NodeKind::Character(c) => {
                    scan_blob_hashes(&c.voice, &mut out);
                    scan_blob_hashes(&c.measurements, &mut out);
                    out.extend(c.materials.iter().filter_map(|m| match &m.content {
                        MaterialContent::Image { blob, .. } => Some(blob.clone()),
                        MaterialContent::Sheet { .. } => None,
                    }));
                }
                _ => {}
            }
        }
        out
    }

    /// Verificação estrutural completa: pais/filhos coerentes, raízes válidas, tipos aceitos, referências existentes.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen_in_roots = std::collections::BTreeSet::new();
        for (cat, list) in self.roots.all() {
            for id in list {
                let n = self.nodes.get(id).ok_or_else(|| format!("raiz {id} não existe"))?;
                if n.parent.is_some() {
                    return Err(format!("raiz {id} tem pai"));
                }
                if n.data.root_category() != Some(cat) {
                    return Err(format!("raiz {id} na categoria errada"));
                }
                if !seen_in_roots.insert(id) {
                    return Err(format!("raiz {id} repetida"));
                }
            }
        }
        for (id, n) in &self.nodes {
            if &n.id != id {
                return Err(format!("id do nó {} não bate com a chave {id}", n.id));
            }
            n.data.validate().map_err(|e| format!("{} ({}): {e}", n.name, id))?;
            match &n.parent {
                None => {
                    if !seen_in_roots.contains(id) {
                        return Err(format!("nó órfão {id}"));
                    }
                }
                Some(p) => {
                    let pn = self.nodes.get(p).ok_or_else(|| format!("pai {p} de {id} não existe"))?;
                    if !pn.children.contains(id) {
                        return Err(format!("pai {p} não lista {id}"));
                    }
                    if !pn.data.accepts_child(&n.data) {
                        return Err(format!("{} não aceita filho {}", pn.data.type_name(), n.data.type_name()));
                    }
                }
            }
            let mut uniq = std::collections::BTreeSet::new();
            for c in &n.children {
                let cn = self.nodes.get(c).ok_or_else(|| format!("filho {c} de {id} não existe"))?;
                if cn.parent.as_ref() != Some(id) {
                    return Err(format!("filho {c} não aponta para {id}"));
                }
                if !uniq.insert(c) {
                    return Err(format!("filho {c} repetido em {id}"));
                }
            }
            if let NodeKind::Layer(l) = &n.data {
                let mut numbers = std::collections::BTreeSet::new();
                for c in &n.children {
                    if let Some(NodeKind::Cel(cel)) = self.nodes.get(c).map(|n| &n.data) {
                        if !numbers.insert(cel.number) {
                            return Err(format!("cel {} repetido na camada {}", cel.number, n.name));
                        }
                    }
                }
                if let Some(m) = &l.mask {
                    if !self.nodes.contains_key(m) {
                        return Err(format!("máscara {m} de {id} não existe"));
                    }
                }
                if let Some(Ok(m3)) = (l.kind == LayerType::Model3d).then(|| crate::model3d::Model3dParams::from_layer_params(&l.params)).flatten() {
                    if !matches!(self.nodes.get(&m3.asset).map(|a| &a.data), Some(NodeKind::Asset(a)) if a.asset == AssetType::Model3d) {
                        return Err(format!("camada 3D {} aponta para um ativo 3D inexistente ({})", n.name, m3.asset));
                    }
                }
                for r in &l.line_refs {
                    if !self.nodes.contains_key(r) {
                        return Err(format!("referência de linha {r} de {id} não existe"));
                    }
                }
            }
            if let NodeKind::Character(c) = &n.data {
                let mut ids = std::collections::BTreeSet::new();
                for m in &c.materials {
                    if !ids.insert(&m.id) {
                        return Err(format!("material {} repetido em {}", m.id, n.name));
                    }
                    let cut = m.content.cut();
                    if !matches!(self.nodes.get(cut), Some(cn) if matches!(cn.data, NodeKind::Cut(_))) {
                        return Err(format!("material {} de {} aponta para um corte inexistente ({cut})", m.name, n.name));
                    }
                }
            }
            if let NodeKind::Cut(c) = &n.data {
                for t in &c.xsheet.tracks {
                    if let Some(l) = &t.layer {
                        if !self.nodes.contains_key(l) {
                            return Err(format!("trilha {} aponta para camada inexistente {l}", t.name));
                        }
                    }
                }
            }
        }
        // Ciclos: cada nó alcança uma raiz subindo pelos pais em no máximo N passos.
        let limit = self.nodes.len() + 1;
        for id in self.nodes.keys() {
            let mut cur = self.nodes.get(id).and_then(|n| n.parent.clone());
            let mut steps = 0;
            while let Some(p) = cur {
                steps += 1;
                if steps > limit {
                    return Err(format!("ciclo na hierarquia passando por {id}"));
                }
                cur = self.nodes.get(&p).and_then(|n| n.parent.clone());
            }
        }
        Ok(())
    }
}
