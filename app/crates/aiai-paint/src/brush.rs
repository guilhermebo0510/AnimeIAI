//! Motor de pincel raster por carimbos ("dabs"), inspirado no painel Brush Settings do Photoshop e no modelo de dab do
//! libmypaint (leitura de comportamento; implementação própria):
//!
//! - ponta procedural (diâmetro, dureza, redondeza, ângulo) ou por máscara de imagem;
//! - espaçamento em fração do diâmetro; dinâmicas de pressão → tamanho, opacidade, fluxo;
//! - dispersão, jitter de tamanho/ângulo/cor (RNG com semente → determinístico), ângulo seguindo a direção;
//! - textura de pincel em espaço de tela (multiplicar/subtrair, escala, profundidade) e pincel duplo;
//! - mistura com a cor da camada (borrar/misturar) e bordas molhadas (aquarela);
//! - duas cores no mesmo carimbo (cenário: luz + sombra da rosa, topo claro e base na sombra da nuvem, lado de
//!   sombra da folha), com a segunda cor derivada da cor do pincel ou escolhida;
//! - acumulação dentro do traço com fluxo, limitada pela opacidade do traço (como o Photoshop).
//!
//! O traço é acumulado num buffer RGBA pré-multiplicado e depois composto na camada com o modo do pincel.

use crate::blend::blend_pixel;
use crate::image::Image;
use crate::mask::Mask;
use crate::noise::{Rng, fbm, value_noise, worley};
use aiai_core::BlendMode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrushTip {
    /// Ponta redonda/elíptica. `hardness` 0 = macia (queda cosseno desde o centro), 1 = borda dura.
    Round { hardness: f32, roundness: f32, angle: f32 },
    /// Máscara em escala de cinza (desenhada no app ou importada) guardada em blob ("AIM1"), esticada para o diâmetro.
    /// `mask` é carregada do blob por quem chama (aiai-tools) antes de pintar.
    /// Forma de cenário (folha, pétala, lâmina de grama, nuvem, estrela, quadrado) com proporção (largura/altura).
    Shape { shape: TipShape, #[serde(default = "d_one")] aspect: f32 },
    Image {
        blob: aiai_core::BlobHash,
        #[serde(skip)]
        #[schemars(skip)]
        mask: Option<Mask>,
    },
}

/// Formas de ponta para pincéis de cenário (folhagem, grama, flores, nuvens, brilhos).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TipShape {
    Leaf,
    Petal,
    GrassBlade,
    CloudPuff,
    Star,
    Square,
    /// Rosa em botão: disco com espiral de pétalas (vãos deixam ver o fundo escuro).
    Rose,
}

/// Cobertura 0..1 de uma forma em coordenadas locais normalizadas (u, v ∈ [-1, 1]; v = -1 é a "ponta" superior).
/// `aa` = largura da borda antisserrilhada em unidades locais (≈ 1/raio_px).
pub fn shape_coverage(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    shape_alpha(shape, u, v, aa)
}

/// Como [`shape_coverage`], com a forma da ponta de duas cores (a rosa vira o botão inteiro): o contorno do cursor.
pub fn shape_coverage_tone(shape: TipShape, u: f32, v: f32, aa: f32, two_tone: bool) -> f32 {
    shape_alpha_tone(shape, u, v, aa, two_tone)
}

fn shape_alpha(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    shape_alpha_tone(shape, u, v, aa, false)
}

/// Raio e fase da espiral de pétalas da rosa (um vinco a cada π).
fn rose_phase(u: f32, v: f32) -> (f32, f32) {
    let r = (u * u + v * v).sqrt();
    let th = v.atan2(u);
    (r, th + 5.5 * r.max(0.05).ln())
}

/// Contorno com 5 lóbulos (as pétalas de fora) da rosa de duas cores.
fn rose_rim(u: f32, v: f32) -> f32 {
    0.86 + 0.07 * (5.0 * v.atan2(u) + 0.6).cos()
}

/// Cobertura da forma; com `two_tone` a rosa é o botão inteiro (os vincos viram sombra em vez de buraco).
fn shape_alpha_tone(shape: TipShape, u: f32, v: f32, aa: f32, two_tone: bool) -> f32 {
    let edge = |d: f32| (0.5 - d / aa.max(1e-4)).clamp(0.0, 1.0); // d = distância com sinal (aprox.) em unidades locais
    match shape {
        TipShape::Leaf => {
            if v.abs() > 1.0 { return 0.0 }
            let half = 0.42 * (1.0 - v * v).max(0.0).powf(0.75);
            edge(u.abs() - half)
        }
        TipShape::Petal => {
            if v.abs() > 1.0 { return 0.0 }
            let t = (v + 1.0) / 2.0; // 0 em cima
            let half = 0.55 * (t * (1.0 - t)).max(0.0).sqrt() * (0.6 + 0.8 * t) ;
            edge(u.abs() - half)
        }
        TipShape::GrassBlade => {
            if v.abs() > 1.0 { return 0.0 }
            let t = (v + 1.0) / 2.0;
            let bend = 0.25 * (1.0 - t) * (1.0 - t);
            let half = 0.12 * t.powf(0.7);
            edge((u - bend).abs() - half)
        }
        TipShape::CloudPuff => {
            // cúmulo: borda de cima definida, base que se desfaz (a borda alarga para baixo)
            let d = cloud_sdf(u, v);
            let soft = 0.5 * (v - 0.05).max(0.0);
            (0.5 - d / (aa.max(1e-4) + soft)).clamp(0.0, 1.0)
        }
        TipShape::Star => {
            let r = (u * u + v * v).sqrt();
            let a = v.atan2(u);
            let k = (a * 5.0 / 2.0).cos().abs().powf(8.0);
            let rad = 0.25 + 0.75 * k;
            edge(r - rad)
        }
        TipShape::Square => edge(u.abs().max(v.abs()) - 0.9),
        TipShape::Rose if two_tone => {
            let (r, _) = rose_phase(u, v);
            if r > 1.0 { return 0.0 }
            edge(r - rose_rim(u, v))
        }
        TipShape::Rose => {
            let r = (u * u + v * v).sqrt();
            if r > 1.0 { return 0.0 }
            let th = v.atan2(u);
            // espiral logarítmica de pétalas: vão fino onde sin(θ + k·ln r) ≈ 0
            let phase = th + 9.0 * (r.max(0.05)).ln();
            let gap = (phase * 1.5).sin().abs();
            let outer = edge(r - 0.92);
            let inner = ((gap - 0.12) / 0.1).clamp(0.0, 1.0);
            outer * if r < 0.12 { 1.0 } else { inner.max(0.15) }
        }
    }
}

fn cloud_sdf(u: f32, v: f32) -> f32 {
    let c = |cx: f32, cy: f32, r: f32| ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r;
    c(0.0, -0.1, 0.55).min(c(-0.45, 0.2, 0.42)).min(c(0.45, 0.2, 0.45)).min(c(0.0, 0.35, 0.5))
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Quanto da segunda cor (sombra) vai no ponto da forma: 0 = cor do pincel, 1 = segunda cor. `aa` = 1 px em
/// unidades locais: carimbo pequeno perde o detalhe fino (senão a espiral vira pontilhado).
fn shape_tone(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
    match shape {
        TipShape::Rose => {
            let (r, phase) = rose_phase(u, v);
            // a pétala de cima faz sombra na de baixo logo depois de cada vinco (meia-lua); a beirada da pétala pega
            // um fio de luz; o miolo fechado é o mais escuro e as pétalas de fora, as mais claras
            let p = (phase / std::f32::consts::PI).rem_euclid(1.0);
            let crescent = (1.0 - p / 0.55).clamp(0.0, 1.0).powf(1.3) * smoothstep(0.0, 0.08, p) * 0.85;
            let core = (1.0 - r / 0.38).clamp(0.0, 1.0);
            let detail = smoothstep(0.17, 0.06, aa);
            let simple = (1.0 - r / 0.8).clamp(0.0, 1.0) * 0.8;
            let rim = smoothstep(0.62, 0.86, r) * 0.6;
            (simple + (crescent.max(core) - simple) * detail - rim).clamp(0.0, 1.0)
        }
        // topo iluminado, base dos lóbulos na sombra
        TipShape::CloudPuff => smoothstep(-0.2, 0.62, v),
        // metade da folha do lado da sombra, nervura no meio
        TipShape::Leaf | TipShape::Petal => smoothstep(-0.04, 0.08, u) * 0.85,
        // base da lâmina escura, ponta clara
        TipShape::GrassBlade => smoothstep(-0.3, 0.95, v),
        TipShape::Star | TipShape::Square => 0.0,
    }
}

/// Duas cores no mesmo carimbo (pincéis de cenário). A segunda cor é `color`, ou a cor do pincel multiplicada por
/// `tint` na proporção `amount` (vale para qualquer cor: rosa vermelha → vinho, nuvem branca → cinza-azulado).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TwoTone {
    #[serde(default = "d_tint")]
    pub tint: [f32; 3],
    /// 0..1
    #[serde(default = "d_one")]
    pub amount: f32,
    /// Segunda cor fixa (sRGB); `None` = derivada da cor do pincel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 3]>,
}

fn d_tint() -> [f32; 3] {
    [0.55, 0.55, 0.7]
}

impl TwoTone {
    pub fn second(&self, rgb: [f32; 3]) -> [f32; 3] {
        match self.color {
            Some(c) => c,
            None => [0, 1, 2].map(|i| rgb[i] * (1.0 - self.amount + self.amount * self.tint[i])),
        }
    }
}

impl Default for BrushTip {
    fn default() -> Self {
        BrushTip::Round { hardness: 0.8, roundness: 1.0, angle: 0.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureKind {
    #[default]
    Paper,
    Canvas,
    Noise,
    Cells,
    Grain,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextureMode {
    #[default]
    Multiply,
    Subtract,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushTexture {
    pub kind: TextureKind,
    /// Tamanho do grão em px.
    pub scale: f32,
    /// 0..1
    pub depth: f32,
    #[serde(default)]
    pub mode: TextureMode,
    #[serde(default)]
    pub seed: u32,
    /// Inverte a textura (os vales viram picos).
    #[serde(default)]
    pub invert: bool,
    /// Textura de imagem (escala de cinza, blob "AIM1"; importada ou feita pelo usuário) no lugar da procedural:
    /// repetida pelo canvas, `scale` = zoom (1 = um pixel da imagem por pixel do canvas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<aiai_core::BlobHash>,
    /// A máscara de `image`, carregada do blob por quem chama (aiai-tools) antes de pintar.
    #[serde(skip)]
    #[schemars(skip)]
    pub image_mask: Option<std::sync::Arc<crate::mask::Mask>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DualBrush {
    pub size: f32,
    pub spacing: f32,
    pub scatter: f32,
    pub hardness: f32,
}

/// Autor de um pincel (vai no manifesto do `.aiaibrush`; tudo opcional).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushAuthor {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

impl BrushAuthor {
    pub fn is_empty(&self) -> bool {
        self.name.trim().is_empty() && self.email.trim().is_empty() && self.url.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushPreset {
    pub name: String,
    /// Categoria na biblioteca (lapis, pintura, aquarela, textura, cenario, borracha ou uma do usuário).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub category: String,
    /// De onde veio (pincel importado: arquivo e pasta dentro dele); a licença é a do autor do pacote.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source: String,
    /// Identificador estável (UUID) do pincel na biblioteca e nos pacotes; vazio = pincel sem arquivo próprio.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "BrushAuthor::is_empty")]
    pub author: BrushAuthor,
    /// Licença: um identificador SPDX (CC-BY-4.0, CC0-1.0, MIT...) ou o texto livre da licença.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub license: String,
    /// Versão do pincel (1.0.0), para a biblioteca saber qual cópia é mais nova.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub version: String,
    /// Criado/alterado em (ISO 8601, UTC).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub tip: BrushTip,
    /// Diâmetro em px com pressão máxima.
    pub size: f32,
    /// Distância entre dabs em fração do diâmetro (0,02..2).
    #[serde(default = "d_spacing")]
    pub spacing: f32,
    /// Opacidade máxima do traço (0..1).
    #[serde(default = "d_one")]
    pub opacity: f32,
    /// Quanto cada dab deposita (0..1).
    #[serde(default = "d_one")]
    pub flow: f32,
    /// Tamanho mínimo com pressão 0 (fração do tamanho). 1 = pressão não muda o tamanho.
    #[serde(default = "d_size_min")]
    pub size_pressure_min: f32,
    /// Opacidade (do dab) mínima com pressão 0.
    #[serde(default = "d_one")]
    pub opacity_pressure_min: f32,
    #[serde(default = "d_one")]
    pub flow_pressure_min: f32,
    /// Deslocamento aleatório perpendicular em fração do diâmetro.
    #[serde(default)]
    pub scatter: f32,
    /// Dabs por passo (com dispersão > 0 cria nuvens de pinceladas).
    #[serde(default = "d_count")]
    pub count: u32,
    #[serde(default)]
    pub size_jitter: f32,
    /// Graus.
    #[serde(default)]
    pub angle_jitter: f32,
    /// A ponta gira seguindo a direção do traço.
    #[serde(default)]
    pub follow_direction: bool,
    /// Jitter de matiz (0..1 da volta), saturação e valor (0..1).
    #[serde(default)]
    pub hue_jitter: f32,
    #[serde(default)]
    pub saturation_jitter: f32,
    #[serde(default)]
    pub value_jitter: f32,
    #[serde(default)]
    pub texture: Option<BrushTexture>,
    #[serde(default)]
    pub dual: Option<DualBrush>,
    /// 0 = só a cor do pincel; 1 = só a cor que já está na camada (borrar).
    #[serde(default)]
    pub mix: f32,
    /// Quanto a cor misturada "arrasta" de um dab para o próximo (borrar).
    #[serde(default)]
    pub smudge_carry: f32,
    /// Bordas molhadas (aquarela): 0..1.
    #[serde(default)]
    pub wet_edges: f32,
    #[serde(default)]
    pub mode: BrushMode,
    /// Sem antisserrilhado (lápis de pixel / linha de cel raster).
    #[serde(default)]
    pub aliased: bool,
    /// Duas cores no carimbo (só pontas de forma: rosa, nuvem, folha, pétala, grama).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub two_tone: Option<TwoTone>,
    /// Cor em degradê AO LONGO DO TRAÇO: no lugar da cor do pincel, cada dab pega a cor do degradê na distância já
    /// percorrida (`length` px = uma passada de 0 a 1; `extend` do degradê decide o que vem depois: prender, repetir,
    /// espelhar). As cores de paleta já chegam resolvidas (o pincel não enxerga o projeto; paleta = magenta).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_ramp: Option<ColorRamp>,
    /// Grupo de ferramenta (abas da prateleira, como no Clip Studio): `caneta`, `marcador`, `lapis`, `pincel`,
    /// `aerografo`, `decoracao`, `borracha`, `mistura` (ou vazio = sem grupo: pincel antigo do usuário).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Suavização de serrilhado em 4 níveis (0 sem, 1 fraca, 2 média, 3 forte, como no Clip Studio); `None` = a borda de
    /// ~1 px de sempre. Com nível, a ponta redonda usa a distância real à borda da elipse (ponta chata fica nítida).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antialias: Option<u8>,
    /// Dinâmicas (pressão/inclinação/velocidade/aleatório com mínimo e curva). Quando presentes, substituem o
    /// `*_pressure_min` do mesmo parâmetro.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_dyn: Option<crate::brush_dyn::Dynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow_dyn: Option<crate::brush_dyn::Dynamics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opacity_dyn: Option<crate::brush_dyn::Dynamics>,
    /// Espessura da ponta (achatamento da redonda: multiplica `roundness`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thickness_dyn: Option<crate::brush_dyn::Dynamics>,
    /// Velocidade (px/s) que conta como 100% nas dinâmicas e no afinamento pela velocidade; 0 = 2500.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub velocity_full: f32,
    /// Entrada e saída (afinamento das pontas do traço).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<crate::brush_dyn::Taper>,
    /// Estabilização do traço deste pincel (0..100, "pela velocidade"); quem desenha à mão usa no lugar da do painel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stabilization: Option<crate::brush_dyn::Stabilization>,
    /// Borda áspera da ponta redonda (canetas texturizada, rugosa, G real).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<crate::brush_dyn::EdgeNoise>,
    /// Tinta pelo máximo: dentro do traço, cada pixel fica com o MAIOR alfa dos carimbos que o tocaram, em vez de somar
    /// camadas (canetas: a borda tem o perfil da ponta e não engrossa nem endurece com a sobreposição dos carimbos).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub max_ink: bool,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Velocidade padrão (px/s) que conta como 100%.
pub const VELOCITY_FULL_DEFAULT: f32 = 2500.0;

/// Degradê do pincel ao longo do comprimento do traço.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ColorRamp {
    pub gradient: aiai_core::Gradient,
    /// Px do caminho para ir de 0 a 1.
    #[serde(default = "d_ramp_len")]
    pub length: f32,
}

fn d_ramp_len() -> f32 {
    400.0
}

impl ColorRamp {
    fn resolved(&self) -> aiai_core::ResolvedGradient {
        self.gradient.resolve(|c| match c {
            aiai_core::ColorRef::Direct { rgba } => rgba.0,
            aiai_core::ColorRef::Palette { .. } => [1.0, 0.0, 1.0, 1.0],
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum BrushMode {
    #[default]
    Normal,
    Erase,
    Behind,
    Multiply,
    Screen,
    Overlay,
    Add,
}

fn d_spacing() -> f32 {
    0.12
}
fn d_one() -> f32 {
    1.0
}
fn d_size_min() -> f32 {
    0.2
}
fn d_count() -> u32 {
    1
}

impl BrushPreset {
    pub fn round(name: &str, size: f32, hardness: f32) -> Self {
        BrushPreset {
            name: name.into(),
            category: String::new(),
            source: String::new(),
            id: String::new(),
            author: BrushAuthor::default(),
            license: String::new(),
            version: String::new(),
            created: String::new(),
            modified: String::new(),
            tags: Vec::new(),
            tip: BrushTip::Round { hardness, roundness: 1.0, angle: 0.0 },
            size,
            spacing: d_spacing(),
            opacity: 1.0,
            flow: 1.0,
            size_pressure_min: 0.2,
            opacity_pressure_min: 1.0,
            flow_pressure_min: 1.0,
            scatter: 0.0,
            count: 1,
            size_jitter: 0.0,
            angle_jitter: 0.0,
            follow_direction: false,
            hue_jitter: 0.0,
            saturation_jitter: 0.0,
            value_jitter: 0.0,
            texture: None,
            dual: None,
            mix: 0.0,
            smudge_carry: 0.0,
            wet_edges: 0.0,
            mode: BrushMode::Normal,
            aliased: false,
            two_tone: None,
            color_ramp: None,
            group: String::new(),
            antialias: None,
            size_dyn: None,
            flow_dyn: None,
            opacity_dyn: None,
            thickness_dyn: None,
            velocity_full: 0.0,
            taper: None,
            stabilization: None,
            edge: None,
            max_ink: false,
        }
    }

    /// Pixels por segundo que contam como velocidade 100%.
    pub fn velocity_full(&self) -> f32 {
        if self.velocity_full > 0.0 { self.velocity_full } else { VELOCITY_FULL_DEFAULT }
    }

    /// Caneta de pixel: 1 px sem antisserrilhado, linha sem degraus duplos (pixel-perfect).
    pub fn is_pixel_pen(&self) -> bool {
        (self.aliased || self.antialias == Some(0)) && self.size <= 1.0 && matches!(self.tip, BrushTip::Round { .. }) && self.mode != BrushMode::Erase
    }

    pub fn validate(&self) -> Result<(), String> {
        let unit = |v: f32, n: &str| if (0.0..=1.0).contains(&v) { Ok(()) } else { Err(format!("{n} fora de 0..1: {v}")) };
        if !(self.size.is_finite() && self.size > 0.0 && self.size <= 2000.0) {
            return Err(format!("tamanho inválido {}", self.size));
        }
        if !(0.01..=5.0).contains(&self.spacing) {
            return Err(format!("espaçamento fora de 0,01..5: {}", self.spacing));
        }
        unit(self.opacity, "opacity")?;
        unit(self.flow, "flow")?;
        unit(self.size_pressure_min, "size_pressure_min")?;
        unit(self.opacity_pressure_min, "opacity_pressure_min")?;
        unit(self.flow_pressure_min, "flow_pressure_min")?;
        unit(self.mix, "mix")?;
        unit(self.smudge_carry, "smudge_carry")?;
        unit(self.wet_edges, "wet_edges")?;
        if let Some(t) = &self.two_tone {
            unit(t.amount, "two_tone.amount")?;
            if t.tint.iter().chain(t.color.iter().flatten()).any(|c| !(0.0..=1.0).contains(c)) {
                return Err("two_tone: cores fora de 0..1".into());
            }
        }
        if self.count == 0 || self.count > 64 {
            return Err("count deve estar entre 1 e 64".into());
        }
        if self.antialias.is_some_and(|a| a > 3) {
            return Err("antialias: 0 (sem), 1 (fraca), 2 (média) ou 3 (forte)".into());
        }
        for (n, d) in [("size_dyn", &self.size_dyn), ("flow_dyn", &self.flow_dyn), ("opacity_dyn", &self.opacity_dyn), ("thickness_dyn", &self.thickness_dyn)] {
            if let Some(d) = d {
                d.validate(n)?;
            }
        }
        if !(self.velocity_full.is_finite() && (0.0..=100_000.0).contains(&self.velocity_full)) {
            return Err(format!("velocity_full fora de 0..100000 px/s: {}", self.velocity_full));
        }
        if let Some(t) = &self.taper {
            t.validate()?;
        }
        if self.stabilization.is_some_and(|s| s.level > 100) {
            return Err("stabilization.level fora de 0..100".into());
        }
        if let Some(e) = &self.edge {
            e.validate()?;
        }
        Ok(())
    }

    /// Borracha redonda padrão (30 px, dureza 0,9): a base da ferramenta Borracha raster. Não faz parte de `defaults()`
    /// (a biblioteca de pincéis não lista borrachas: a ferramenta Borracha cobre); o nome "Borracha" ainda vale no PAINT
    /// da bridge, e os traços já gravados guardam o preset dentro deles.
    pub fn eraser() -> BrushPreset {
        let mut p = BrushPreset::round("Borracha", 30.0, 0.9);
        p.mode = BrushMode::Erase;
        p
    }

    /// Presets iniciais do programa (S06).
    pub fn defaults() -> Vec<BrushPreset> {
        let mut v = Vec::new();
        let mut pencil = BrushPreset::round("Lápis de rascunho", 3.0, 0.9);
        pencil.opacity_pressure_min = 0.3;
        pencil.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 1.5, depth: 0.6, mode: TextureMode::Multiply, seed: 3, invert: false, image: None, image_mask: None });
        v.push(pencil);
        v.push(BrushPreset::round("Redondo duro", 24.0, 0.95));
        let mut soft = BrushPreset::round("Redondo macio", 60.0, 0.0);
        soft.flow = 0.35;
        soft.size_pressure_min = 0.6;
        v.push(soft);
        let mut air = BrushPreset::round("Aerógrafo", 120.0, 0.0);
        air.flow = 0.08;
        air.opacity = 0.8;
        air.size_pressure_min = 1.0;
        air.spacing = 0.05;
        v.push(air);
        let mut wc = BrushPreset::round("Aquarela", 50.0, 0.5);
        wc.flow = 0.5;
        wc.opacity = 0.7;
        wc.wet_edges = 0.7;
        wc.mix = 0.15;
        wc.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 6.0, depth: 0.35, mode: TextureMode::Multiply, seed: 7, invert: false, image: None, image_mask: None });
        v.push(wc);
        let mut gouache = BrushPreset::round("Guache seco", 36.0, 0.75);
        gouache.tip = BrushTip::Round { hardness: 0.75, roundness: 0.55, angle: 35.0 };
        gouache.follow_direction = true;
        gouache.texture = Some(BrushTexture { kind: TextureKind::Canvas, scale: 3.0, depth: 0.55, mode: TextureMode::Subtract, seed: 11, invert: false, image: None, image_mask: None });
        gouache.opacity_pressure_min = 0.5;
        v.push(gouache);
        let mut charcoal = BrushPreset::round("Carvão", 14.0, 0.6);
        charcoal.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 2.5, depth: 0.85, mode: TextureMode::Subtract, seed: 5, invert: false, image: None, image_mask: None });
        charcoal.scatter = 0.1;
        charcoal.opacity_pressure_min = 0.2;
        v.push(charcoal);
        let mut paper = BrushPreset::round("Textura de papel", 80.0, 0.3);
        paper.flow = 0.4;
        paper.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 10.0, depth: 0.9, mode: TextureMode::Multiply, seed: 13, invert: false, image: None, image_mask: None });
        v.push(paper);
        let mut smudge = BrushPreset::round("Borrador", 30.0, 0.4);
        smudge.mix = 0.95;
        smudge.smudge_carry = 0.8;
        smudge.flow = 0.6;
        v.push(smudge);
        // pincéis de cenário (S11)
        let mut foliage = BrushPreset::round("Folhagem", 26.0, 1.0);
        foliage.tip = BrushTip::Shape { shape: TipShape::Leaf, aspect: 0.8 };
        foliage.spacing = 0.35;
        foliage.scatter = 1.2;
        foliage.count = 3;
        foliage.size_jitter = 0.4;
        foliage.angle_jitter = 180.0;
        foliage.value_jitter = 0.12;
        foliage.hue_jitter = 0.04;
        foliage.size_pressure_min = 0.6;
        foliage.two_tone = Some(TwoTone { tint: [0.45, 0.6, 0.62], amount: 0.7, color: None });
        v.push(foliage);
        let mut grass = BrushPreset::round("Grama", 30.0, 1.0);
        grass.tip = BrushTip::Shape { shape: TipShape::GrassBlade, aspect: 1.0 };
        grass.spacing = 0.12;
        grass.scatter = 0.3;
        grass.count = 2;
        grass.size_jitter = 0.35;
        grass.angle_jitter = 12.0;
        grass.value_jitter = 0.15;
        grass.two_tone = Some(TwoTone { tint: [0.4, 0.58, 0.5], amount: 0.65, color: None });
        v.push(grass);
        let mut rose = BrushPreset::round("Rosas", 18.0, 1.0);
        rose.tip = BrushTip::Shape { shape: TipShape::Rose, aspect: 1.0 };
        rose.spacing = 0.9;
        rose.scatter = 0.8;
        rose.count = 2;
        rose.size_jitter = 0.35;
        rose.angle_jitter = 180.0;
        rose.value_jitter = 0.1;
        // luz nas pétalas de fora, sombra vinho nos vincos e no miolo
        rose.two_tone = Some(TwoTone { tint: [0.42, 0.16, 0.3], amount: 0.85, color: None });
        v.push(rose);
        let mut petals = BrushPreset::round("Flores/pétalas", 14.0, 1.0);
        petals.tip = BrushTip::Shape { shape: TipShape::Petal, aspect: 0.8 };
        petals.spacing = 0.5;
        petals.scatter = 1.5;
        petals.count = 3;
        petals.size_jitter = 0.4;
        petals.angle_jitter = 180.0;
        petals.saturation_jitter = 0.1;
        petals.value_jitter = 0.12;
        v.push(petals);
        let mut cloud = BrushPreset::round("Nuvem", 90.0, 1.0);
        cloud.tip = BrushTip::Shape { shape: TipShape::CloudPuff, aspect: 1.2 };
        cloud.spacing = 0.25;
        cloud.scatter = 0.25;
        cloud.size_jitter = 0.3;
        // fluxo cheio: carimbos semitransparentes empilhados faziam escamas dentro da nuvem; a transparência fica
        // na opacidade do traço, e o volume vem da base de cada lóbulo na sombra
        cloud.flow = 1.0;
        cloud.opacity = 0.95;
        cloud.two_tone = Some(TwoTone { tint: [0.66, 0.72, 0.88], amount: 1.0, color: None });
        v.push(cloud);
        let mut sparkle = BrushPreset::round("Brilhos", 18.0, 1.0);
        sparkle.tip = BrushTip::Shape { shape: TipShape::Star, aspect: 1.0 };
        sparkle.spacing = 1.5;
        sparkle.scatter = 2.0;
        sparkle.size_jitter = 0.6;
        sparkle.angle_jitter = 20.0;
        v.push(sparkle);
        // sem "Borracha" na biblioteca de fábrica: a ferramenta Borracha cobre (ver `BrushPreset::eraser`)
        for p in &mut v {
            p.category = match p.name.as_str() {
                "Lápis de rascunho" => "lapis",
                "Redondo duro" | "Redondo macio" | "Aerógrafo" | "Guache seco" => "pintura",
                "Aquarela" | "Borrador" => "aquarela",
                "Carvão" | "Textura de papel" => "textura",
                _ => "cenario",
            }
            .into();
            p.group = match p.name.as_str() {
                "Lápis de rascunho" | "Carvão" => "lapis",
                "Aerógrafo" => "aerografo",
                "Borrador" => "mistura",
                "Redondo duro" | "Redondo macio" | "Aquarela" | "Guache seco" | "Textura de papel" => "pincel",
                _ => "decoracao",
            }
            .into();
        }
        v.extend(Self::pens());
        v
    }

    /// Canetas e marcadores de fábrica (S25): o resultado calibrado nas canetas de fábrica do Clip Studio Paint 5.1.5
    /// (folha de teste em 3, 10 e 30 px; `tools/csp/calib_compare.py`), com os números do banco de ferramentas dele
    /// (tamanho padrão, curvas de pressão, mínimos, velocidade, espessura e ângulo da ponta, suavização de serrilhado,
    /// estabilização). Nomes das penas reais; pontas e bordas desenhadas pelo nosso motor.
    pub fn pens() -> Vec<BrushPreset> {
        use crate::brush_dyn::{DynInput, Dynamics, EdgeNoise, Stabilization};
        let curve = |min: f32, pts: &[[f32; 2]]| DynInput { min, curve: pts.to_vec() };
        let stab = |level: u8| Some(Stabilization { level, by_speed: true });
        let pen = |name: &str, size: f32, group: &str| {
            let mut p = BrushPreset::round(name, size, 1.0);
            p.group = group.into();
            p.category = "lapis".into();
            p.antialias = Some(2);
            p.spacing = 0.08;
            p.size_pressure_min = 1.0;
            p.max_ink = true;
            p
        };
        let mut v = Vec::new();
        // pena G: pressão do zero ao tamanho cheio, linear
        let mut g = pen("Pena G", 10.0, "caneta");
        g.size_dyn = Some(Dynamics::pressure(0.0));
        g.stabilization = stab(6);
        v.push(g);
        // pena G real: ponta levemente achatada girando ao acaso, borda granulada; a espessura também cai com pouca pressão
        let mut gr = pen("Pena G real", 10.0, "caneta");
        gr.tip = BrushTip::Round { hardness: 1.0, roundness: 0.85, angle: 0.0 };
        gr.angle_jitter = 180.0;
        gr.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.745, 0.255], [1.0, 1.0]])), ..Default::default() });
        gr.thickness_dyn = Some(Dynamics { pressure: Some(curve(0.5, &[[0.0, 0.0], [0.345, 0.0], [0.664, 0.955], [1.0, 1.0]])), ..Default::default() });
        gr.edge = Some(EdgeNoise { amount: 0.22, grain: 4.0, seed: 0 });
        gr.stabilization = stab(6);
        v.push(gr);
        // pena de mapeamento: linha fina que só engorda com pressão forte
        let mut m = pen("Pena de mapeamento", 10.0, "caneta");
        m.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.5, 0.245], [0.927, 0.709], [1.0, 1.0]])), ..Default::default() });
        m.stabilization = stab(10);
        v.push(m);
        // pena nabo: engorda logo no começo da pressão
        let mut t = pen("Pena nabo", 8.0, "caneta");
        t.size_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.182, 0.4], [0.7, 0.964], [1.0, 1.0]])), ..Default::default() });
        t.stabilization = stab(2);
        v.push(t);
        // caligrafia: ponta chata fina inclinada (fino na diagonal "/", grosso na "\"), sem pressão
        let mut c = pen("Caligrafia", 50.0, "caneta");
        c.tip = BrushTip::Round { hardness: 1.0, roundness: 0.2, angle: 148.0 };
        c.stabilization = stab(6);
        v.push(c);
        // caneta texturizada: ponta macia achatada girando ao acaso (borda em gomos, desfocada), tinta que soma
        let mut tx = pen("Caneta texturizada", 12.0, "caneta");
        tx.tip = BrushTip::Round { hardness: 0.8, roundness: 0.6, angle: 0.0 };
        tx.angle_jitter = 180.0;
        tx.antialias = Some(1);
        tx.max_ink = false;
        tx.size_dyn = Some(Dynamics::pressure(0.0));
        tx.stabilization = stab(6);
        v.push(tx);
        // caneta rugosa: fita áspera que segue a direção, largura variando aos trechos, afina um pouco quando rápida
        let mut r = pen("Caneta rugosa", 1.2, "caneta");
        r.follow_direction = true;
        r.antialias = Some(1);
        r.size_dyn = Some(Dynamics { pressure: Some(DynInput::linear(0.0)), velocity: Some(DynInput::slower_is_bigger(0.5)), random: Some(0.8), random_len: 40.0, ..Default::default() });
        r.thickness_dyn = Some(Dynamics { velocity: Some(DynInput::slower_is_bigger(0.5)), random: Some(0.85), random_len: 30.0, ..Default::default() });
        r.velocity_full = 25000.0;
        r.edge = Some(EdgeNoise { amount: 0.3, grain: 4.0, seed: 0 });
        r.stabilization = stab(6);
        v.push(r);
        // caneta milimétrica: espessura quase fixa, afina um pouco no traço rápido
        let mut mm = pen("Caneta milimétrica", 10.0, "marcador");
        mm.size_dyn = Some(Dynamics { pressure: Some(curve(0.8, &[[0.0, 0.0], [0.718, 0.309], [1.0, 1.0]])), velocity: Some(DynInput::slower_is_bigger(0.6)), ..Default::default() });
        mm.velocity_full = 13000.0;
        v.push(mm);
        // caneta de feltro: ponta larga deitada; rápida fica fina e falhada (pouca tinta), lenta fica cheia
        let mut f = pen("Caneta de feltro", 20.0, "marcador");
        f.tip = BrushTip::Round { hardness: 1.0, roundness: 0.8, angle: 0.0 };
        f.antialias = Some(1);
        f.size_dyn = Some(Dynamics { pressure: Some(DynInput::linear(0.7)), velocity: Some(DynInput::slower_is_bigger(0.7)), random: Some(0.9), ..Default::default() });
        f.flow_dyn = Some(Dynamics { velocity: Some(DynInput::slower_is_bigger(0.18)), ..Default::default() });
        f.velocity_full = 2500.0;
        f.max_ink = false;
        f.spacing = 0.3;
        f.stabilization = stab(6);
        v.push(f);
        // caneta de pixel: 1 px sem antisserrilhado, sem degrau dobrado
        let mut px = pen("Caneta de pixel", 1.0, "marcador");
        px.aliased = true;
        px.antialias = Some(0);
        px.spacing = 0.1;
        v.push(px);
        // marcador chato: ponta chata deitada (traço horizontal fino, vertical largo); a pressão mexe no tamanho e na opacidade
        let mut fm = pen("Marcador chato", 50.0, "marcador");
        fm.tip = BrushTip::Round { hardness: 1.0, roundness: 0.3, angle: 0.0 };
        fm.antialias = Some(1);
        fm.spacing = 0.03;
        fm.size_dyn = Some(Dynamics { pressure: Some(curve(0.6, &[[0.0, 0.0], [0.557, 0.061], [0.934, 0.601], [1.0, 1.0]])), ..Default::default() });
        fm.opacity_dyn = Some(Dynamics { pressure: Some(curve(0.0, &[[0.0, 0.0], [0.588, 0.0], [0.921, 0.675], [1.0, 1.0]])), ..Default::default() });
        v.push(fm);
        v
    }
}

/// Amostra do caminho do pincel (canvas px).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushPoint {
    pub x: f32,
    pub y: f32,
    #[serde(default = "d_one")]
    pub pressure: f32,
}

fn texture_value(t: &BrushTexture, x: f32, y: f32) -> f32 {
    if let Some(m) = t.image_mask.as_deref().filter(|m| m.width > 0 && m.height > 0) {
        // imagem repetida, bilinear
        let z = t.scale.max(0.05);
        let (u, v) = (x / z, y / z);
        let (x0, y0) = (u.floor(), v.floor());
        let (fx, fy) = (u - x0, v - y0);
        let at = |xx: f32, yy: f32| m.get((xx as i64).rem_euclid(m.width as i64) as u32, (yy as i64).rem_euclid(m.height as i64) as u32);
        let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx;
        let bot = at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx;
        let r = top * (1.0 - fy) + bot * fy;
        return if t.invert { 1.0 - r } else { r };
    }
    let s = t.scale.max(0.25);
    let (u, v) = (x / s, y / s);
    let raw = match t.kind {
        TextureKind::Paper => 0.55 * fbm(u * 0.35, v * 0.35, 4, 2.0, 0.55, t.seed) + 0.45 * value_noise(u * 1.7, v * 1.7, t.seed ^ 77),
        TextureKind::Canvas => {
            // trama: senos cruzados + ruído
            let wx = (u * std::f32::consts::PI).sin().abs();
            let wy = (v * std::f32::consts::PI).sin().abs();
            0.35 + 0.35 * wx.max(wy) + 0.3 * value_noise(u * 2.0, v * 2.0, t.seed)
        }
        TextureKind::Noise => fbm(u * 0.25, v * 0.25, 5, 2.0, 0.5, t.seed),
        TextureKind::Cells => worley(u * 0.3, v * 0.3, t.seed),
        TextureKind::Grain => value_noise(u, v, t.seed),
    };
    let r = raw.clamp(0.0, 1.0);
    if t.invert { 1.0 - r } else { r }
}

/// Coordenadas locais (u, v) da ponta de forma, ou `None` se a ponta não é de forma.
fn shape_uv(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32) -> Option<(TipShape, f32, f32)> {
    let BrushTip::Shape { shape, aspect } = tip else { return None };
    let (sn, cs) = extra_angle.to_radians().sin_cos();
    let (lx, ly) = (dx * cs + dy * sn, -dx * sn + dy * cs);
    let r = radius.max(0.25);
    Some((*shape, lx / (r * aspect.clamp(0.1, 4.0)), ly / r))
}

fn tip_alpha(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aliased: bool) -> f32 {
    tip_alpha_tone(tip, dx, dy, radius, extra_angle, aliased, false)
}

fn tip_alpha_tone(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aliased: bool, two_tone: bool) -> f32 {
    match tip {
        BrushTip::Round { hardness, roundness, angle } => {
            let a = (angle + extra_angle).to_radians();
            let (s, c) = a.sin_cos();
            let lx = dx * c + dy * s;
            let ly = (-dx * s + dy * c) / roundness.clamp(0.05, 1.0);
            let d = (lx * lx + ly * ly).sqrt() / radius.max(0.25);
            if aliased {
                return if d <= 1.0 { 1.0 } else { 0.0 };
            }
            // borda antisserrilhada em ~1 px
            let edge = ((1.0 - d) * radius).clamp(0.0, 1.0);
            let h = hardness.clamp(0.0, 1.0);
            let inner = if d <= h { 1.0 } else { let t = ((d - h) / (1.0 - h).max(1e-4)).clamp(0.0, 1.0); 0.5 + 0.5 * (t * std::f32::consts::PI).cos() };
            inner * edge
        }
        BrushTip::Shape { .. } => {
            let (shape, u, v) = shape_uv(tip, dx, dy, radius, extra_angle).expect("ponta de forma");
            let val = shape_alpha_tone(shape, u, v, 1.0 / radius.max(0.25), two_tone);
            if aliased { if val >= 0.5 { 1.0 } else { 0.0 } } else { val }
        }
        BrushTip::Image { mask, .. } => {
            let Some(mask) = mask else { return 0.0 };
            let a = extra_angle.to_radians();
            let (s, c) = a.sin_cos();
            let lx = dx * c + dy * s;
            let ly = -dx * s + dy * c;
            let u = (lx / (2.0 * radius) + 0.5) * mask.width as f32;
            let v = (ly / (2.0 * radius) + 0.5) * mask.height as f32;
            if u < 0.0 || v < 0.0 || u >= mask.width as f32 || v >= mask.height as f32 {
                return 0.0;
            }
            let val = mask.get(u as u32, v as u32);
            if aliased { if val >= 0.5 { 1.0 } else { 0.0 } } else { val }
        }
    }
}

/// Ponta redonda do modelo de caneta (S25): borda pela distância real à elipse (a ponta chata fica nítida na lateral fina),
/// rampa do antisserrilhado no nível pedido (`aa_ramp`), dureza, espessura extra (`thick` multiplica a redondeza) e borda
/// áspera. `None` para pontas que não são redondas (seguem a cobertura de sempre).
fn tip_alpha_pen(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aa: u8, thick: f32, edge_k: f32, rough: bool) -> Option<f32> {
    let BrushTip::Round { hardness, roundness, angle } = tip else { return None };
    let a = (angle + extra_angle).to_radians();
    let (s, c) = a.sin_cos();
    let lx = dx * c + dy * s;
    let ly = -dx * s + dy * c;
    let mut rr = radius.max(0.25);
    let rho = (roundness * thick).clamp(0.02, 1.0).max((crate::brush_dyn::min_semi_axis() / rr).min(1.0));
    rr *= edge_k;
    let (ux, uy) = (lx / rr, ly / (rr * rho));
    let q = (ux * ux + uy * uy).sqrt();
    // distância (px) à borda da elipse ≈ (q - 1) / |∇q|
    let g = ((lx / (rr * rr)).powi(2) + (ly / (rr * rr * rho * rho)).powi(2)).sqrt() / q.max(1e-6);
    let sd = if q < 1e-6 { -rr * rho } else { (q - 1.0) / g.max(1e-6) };
    let edge_cov = if aa == 0 {
        if sd <= 0.0 { 1.0 } else { 0.0 }
    } else {
        // ponta áspera (no Clip Studio é uma imagem amostrada): a borda não recua, só a rampa
        let (w, shift) = crate::brush_dyn::aa_ramp(aa);
        let shift = if edge_k < 1.0 || rough { 0.0 } else { shift };
        ((-sd - shift) / w + 0.5).clamp(0.0, 1.0)
    };
    let h = hardness.clamp(0.0, 1.0);
    let inner = if q <= h || h >= 1.0 { 1.0 } else { let t = ((q - h) / (1.0 - h).max(1e-4)).clamp(0.0, 1.0); 0.5 + 0.5 * (t * std::f32::consts::PI).cos() };
    Some(inner * edge_cov)
}

fn rgb_to_hsv(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (c[0], c[1], c[2]);
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let d = mx - mn;
    let h = if d <= 0.0 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    [h, if mx > 0.0 { d / mx } else { 0.0 }, mx]
}

fn hsv_to_rgb(c: [f32; 3]) -> [f32; 3] {
    let (h, s, v) = (c[0].rem_euclid(1.0) * 6.0, c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0));
    let i = h.floor();
    let f = h - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Resultado: caixa afetada [x0, y0, x1, y1) (para recortar em tiles).
pub struct PaintResult {
    pub bounds: [i32; 4],
}

/// Pinta um traço na `layer` (pré-multiplicada, tamanho do canvas). `color` = sRGB não pré-multiplicado com alfa.
pub fn paint_stroke(layer: &mut Image, preset: &BrushPreset, path: &[BrushPoint], color: [f32; 4], seed: u64) -> Option<PaintResult> {
    paint_stroke_at(layer, [0, 0], preset, path, color, seed)
}

/// Como [`paint_stroke`], com `layer` sendo um recorte do canvas que começa em `origin` (os pontos do caminho já vêm
/// em coordenadas do recorte); só a textura do papel usa a posição absoluta, para o grão não andar com o recorte.
pub fn paint_stroke_at(layer: &mut Image, origin: [i32; 2], preset: &BrushPreset, path: &[BrushPoint], color: [f32; 4], seed: u64) -> Option<PaintResult> {
    if path.is_empty() {
        return None;
    }
    let mut sb = StrokeBuffer::new(preset, color, seed, layer.width, layer.height, origin);
    // o borrador lê a camada de antes do traço (o buffer do traço é composto só no fim)
    sb.add(Some(&*layer), path);
    sb.finish(Some(&*layer));
    sb.composite(layer, [0, 0])
}

// ------------------------------------------------------------------------------------------------ traço incremental

const STS: i32 = 64;

/// Dab gerado e ainda não carimbado (o primeiro espera a direção do primeiro trecho, como no traço inteiro).
#[derive(Clone, Copy)]
struct Dab {
    x: f32,
    y: f32,
    pr: f32,
    dir: f32,
    /// Distância percorrida no caminho até este dab (px), para o degradê ao longo do traço e o afinamento.
    s: f32,
    /// Velocidade normalizada (0..1) no momento do dab.
    v: f32,
    /// Fator de afinamento já conhecido (entrada); a saída multiplica ao carimbar.
    taper: f32,
}

/// Traço de pincel em andamento, alimentado aos pedaços: guarda o buffer do traço (em tiles), o gerador aleatório, o
/// espaçamento acumulado, a direção suavizada e a pegada do borrador entre uma chamada e outra. Alimentar o caminho
/// inteiro de uma vez ou em pedaços dá o MESMO resultado (é o mesmo laço, com o estado guardado): a prévia ao vivo
/// do app carimba só o trecho novo a cada movimento e o commit passa pelo mesmo código.
#[derive(Clone)]
pub struct StrokeBuffer {
    preset: BrushPreset,
    color: [f32; 4],
    /// Limites da camada (coordenadas locais) e deslocamento da textura do papel (posição absoluta no canvas).
    w: i32,
    h: i32,
    tex_origin: [i32; 2],
    rng: Rng,
    drng: Rng,
    /// Tiles compartilhados (`Arc`): a cópia para a prévia do fim do traço custa só os ponteiros; quem escreve num tile
    /// compartilhado ganha a própria cópia dele.
    tiles: std::collections::HashMap<(i32, i32), std::sync::Arc<Vec<[f32; 4]>>>,
    dual: Option<std::collections::HashMap<(i32, i32), std::sync::Arc<Vec<f32>>>>,
    /// Retângulo [x0, y0, x1, y1) carimbado desde o último `take_dirty` (prévia: só ele é recomposto).
    dirty: Option<[i32; 4]>,
    /// Maior alfa do buffer (o nível da borda molhada; o alfa de um pixel do traço só sobe).
    max_a: f32,
    carry: Option<(i32, Vec<[f32; 4]>)>,
    prev: Option<(BrushPoint, f32)>,
    acc: f32,
    sdir: Option<f32>,
    pending: Option<Dab>,
    /// Degradê ao longo do traço (`BrushPreset::color_ramp`) e o comprimento de uma passada.
    ramp: Option<(aiai_core::ResolvedGradient, f32)>,
    /// Comprimento do caminho já percorrido (px).
    travel: f32,
    /// Velocidade do traço (só com amostras com tempo; sem tempo, conta como parado).
    speed: crate::brush_dyn::SpeedEstimator,
    /// Dabs dos últimos `taper.end` px, à espera de saber onde o traço termina (saída).
    held: std::collections::VecDeque<Dab>,
    /// Caneta de pixel: pixels ainda não carimbados (o do meio de um degrau em L sai).
    pix: Vec<(i32, i32)>,
    pix_last: Option<(i32, i32)>,
    /// Semente do aleatório suave ao longo do traço.
    noise_seed: u32,
}

impl StrokeBuffer {
    /// Traço numa camada `w`×`h` (coordenadas locais); `tex_origin` = posição do canto da camada no canvas (textura).
    pub fn new(preset: &BrushPreset, color: [f32; 4], seed: u64, w: u32, h: u32, tex_origin: [i32; 2]) -> Self {
        StrokeBuffer {
            preset: preset.clone(),
            color,
            w: w as i32,
            h: h as i32,
            tex_origin,
            rng: Rng::new(seed),
            drng: Rng::new(seed ^ 0xD0A1),
            tiles: Default::default(),
            dual: preset.dual.as_ref().map(|_| Default::default()),
            carry: None,
            prev: None,
            acc: 0.0,
            sdir: None,
            pending: None,
            dirty: None,
            max_a: 0.0,
            ramp: preset.color_ramp.as_ref().map(|r| (r.resolved(), r.length.max(1.0))),
            travel: 0.0,
            speed: Default::default(),
            held: Default::default(),
            pix: Vec::new(),
            pix_last: None,
            noise_seed: (seed ^ (seed >> 32)) as u32,
        }
    }

    /// Retângulo carimbado desde a última chamada (coordenadas locais).
    pub fn take_dirty(&mut self) -> Option<[i32; 4]> {
        self.dirty.take()
    }

    fn mark(&mut self, r: [i32; 4]) {
        self.dirty = Some(match self.dirty {
            Some(d) => [d[0].min(r[0]), d[1].min(r[1]), d[2].max(r[2]), d[3].max(r[3])],
            None => r,
        });
    }

    fn buf_get(&self, x: i32, y: i32) -> [f32; 4] {
        match self.tiles.get(&(x.div_euclid(STS), y.div_euclid(STS))) {
            Some(t) => t[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize],
            None => [0.0; 4],
        }
    }

    fn buf_mut(&mut self, x: i32, y: i32) -> &mut [f32; 4] {
        let t = self.tiles.entry((x.div_euclid(STS), y.div_euclid(STS))).or_insert_with(|| std::sync::Arc::new(vec![[0.0f32; 4]; (STS * STS) as usize]));
        &mut std::sync::Arc::make_mut(t)[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize]
    }

    /// Mais pontos do caminho (coordenadas locais), sem tempo (a velocidade conta como parada). `layer` = a camada antes
    /// do traço, só para o borrador (mix > 0).
    pub fn add(&mut self, layer: Option<&Image>, pts: &[BrushPoint]) {
        for &p in pts {
            self.push_point(layer, p, f32::NAN);
        }
        self.flush_held(layer);
    }

    /// Como [`add`](Self::add), com o tempo (s desde o começo do traço) de cada ponto: liga as dinâmicas de velocidade
    /// e o afinamento pela velocidade. `times` tem o mesmo tamanho de `pts`.
    pub fn add_timed(&mut self, layer: Option<&Image>, pts: &[BrushPoint], times: &[f32]) {
        for (i, &p) in pts.iter().enumerate() {
            self.push_point(layer, p, times.get(i).copied().unwrap_or(f32::NAN));
        }
        self.flush_held(layer);
    }

    fn push_point(&mut self, layer: Option<&Image>, p: BrushPoint, t: f32) {
        match self.prev {
            None => {
                self.pending = Some(Dab { x: p.x, y: p.y, pr: p.pressure, dir: 0.0, s: 0.0, v: 0.0, taper: self.start_taper(0.0, 0.0) });
                if t.is_finite() {
                    self.speed.push(0.0, t);
                }
            }
            Some((a, ta)) => self.segment(layer, a, ta, p, t),
        }
        self.prev = Some((p, t));
    }

    /// Fim do caminho: carimba o primeiro dab se o caminho não andou e os dabs da saída com o afinamento.
    pub fn finish(&mut self, layer: Option<&Image>) {
        if let Some(d) = self.pending.take() {
            self.emit(layer, d);
        }
        if let Some(tp) = self.preset.taper.clone().filter(|t| t.end > 0.0) {
            let len = if tp.by_speed { tp.end * self.vnorm(self.speed.speed()) } else { tp.end };
            let total = self.travel;
            while let Some(mut d) = self.held.pop_front() {
                d.taper *= tp.ramp(total - d.s, len);
                self.stamp(layer, d);
            }
        }
        for c in std::mem::take(&mut self.pix) {
            self.stamp_pixel(c);
        }
    }

    fn vnorm(&self, speed: f32) -> f32 {
        (speed / self.preset.velocity_full()).clamp(0.0, 1.0)
    }

    /// Fator da entrada na distância `s` do começo (velocidade normalizada `v` para "pela velocidade").
    fn start_taper(&self, s: f32, v: f32) -> f32 {
        match &self.preset.taper {
            Some(tp) if tp.start > 0.0 => {
                let len = if tp.by_speed { tp.start * v } else { tp.start };
                tp.ramp(s, len)
            }
            _ => 1.0,
        }
    }

    /// Dab pronto: carimba, ou espera a saída (os últimos `taper.end` px).
    fn emit(&mut self, layer: Option<&Image>, d: Dab) {
        if self.preset.taper.as_ref().is_some_and(|t| t.end > 0.0) {
            self.held.push_back(d);
        } else {
            self.stamp(layer, d);
        }
    }

    /// Carimba os dabs que já estão longe o bastante do fim para não pegar a saída.
    fn flush_held(&mut self, layer: Option<&Image>) {
        let Some(end) = self.preset.taper.as_ref().map(|t| t.end).filter(|e| *e > 0.0) else { return };
        while self.held.front().is_some_and(|d| self.travel - d.s > end) {
            let d = self.held.pop_front().expect("há dab");
            self.stamp(layer, d);
        }
    }

    /// Tamanho do dab sem o aleatório (espaçamento entre carimbos).
    fn base_diam(&self, pr: f32, v: f32, taper: f32) -> f32 {
        let p = &self.preset;
        let f = match &p.size_dyn {
            Some(d) => d.base(pr, v),
            None => p.size_pressure_min + (1.0 - p.size_pressure_min) * pr.clamp(0.0, 1.0),
        };
        let tsz = if p.taper.as_ref().is_some_and(|t| t.size) { taper } else { 1.0 };
        p.size * f * tsz
    }

    fn segment(&mut self, layer: Option<&Image>, a: BrushPoint, ta: f32, b: BrushPoint, tb: f32) {
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        // pincel duplo: segunda ponta ao longo do caminho (máximo das coberturas); o traço só aparece onde ela cobre
        if let Some(dual) = self.preset.dual.clone() {
            let r = (dual.size * 0.5).max(0.3);
            let step = (dual.size * dual.spacing).max(0.5);
            let n = (len / step).ceil().max(1.0) as usize;
            let tip = BrushTip::Round { hardness: dual.hardness, roundness: 1.0, angle: 0.0 };
            for k in 0..n {
                let f = k as f32 / n as f32;
                let px = a.x + (b.x - a.x) * f + dual.scatter * dual.size * self.drng.signed();
                let py = a.y + (b.y - a.y) * f + dual.scatter * dual.size * self.drng.signed();
                let (dx0, dy0, dx1, dy1) = (((px - r - 1.0) as i32).max(0), ((py - r - 1.0) as i32).max(0), ((px + r + 1.0) as i32 + 1).min(self.w), ((py + r + 1.0) as i32 + 1).min(self.h));
                if dx0 < dx1 && dy0 < dy1 {
                    self.mark([dx0, dy0, dx1, dy1]);
                }
                for y in dy0..dy1 {
                    for x in dx0..dx1 {
                        let a2 = tip_alpha(&tip, x as f32 + 0.5 - px, y as f32 + 0.5 - py, r, 0.0, false);
                        let db = self.dual.as_mut().expect("pincel duplo");
                        let t = std::sync::Arc::make_mut(db.entry((x.div_euclid(STS), y.div_euclid(STS))).or_insert_with(|| std::sync::Arc::new(vec![0.0f32; (STS * STS) as usize])));
                        let i = (y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize;
                        t[i] = t[i].max(a2);
                    }
                }
            }
        }
        if len <= 1e-6 {
            return;
        }
        let sp = if ta.is_finite() && tb.is_finite() { self.speed.push(len, tb) } else { self.speed.speed() };
        let v = self.vnorm(sp);
        // caneta de pixel: os pixels por onde o caminho passa, um por vez, sem o degrau duplo
        if self.preset.is_pixel_pen() {
            if let Some(d) = self.pending.take() {
                self.pixel_at(d.x, d.y);
            }
            let n = (len * 4.0).ceil().max(1.0) as usize;
            for k in 1..=n {
                let f = k as f32 / n as f32;
                self.pixel_at(a.x + dx * f, a.y + dy * f);
            }
            self.travel += len;
            return;
        }
        // direção com passa-baixa (≈ meio diâmetro de memória): a ponta elíptica não gira aos trancos nas microparadas
        let memory = (self.preset.size * 0.5).max(1.0);
        let raw = dy.atan2(dx).to_degrees();
        let dir = match self.sdir {
            None => raw,
            Some(prev) => {
                let d = (raw - prev + 540.0).rem_euclid(360.0) - 180.0;
                prev + d * (1.0 - (-len / memory).exp())
            }
        };
        self.sdir = Some(dir);
        if let Some(pd) = &mut self.pending {
            pd.dir = dir;
        }
        // com saída, os dabs do fim encolhem: o passo acompanha o menor tamanho possível (sem contas)
        let end_k = self.preset.taper.as_ref().filter(|t| t.end > 0.0 && t.size).map(|t| t.min.max(0.2)).unwrap_or(1.0);
        let mut t = 0.0f32;
        loop {
            let pr = a.pressure + (b.pressure - a.pressure) * (t / len);
            let s_here = self.travel + t;
            let diam = self.base_diam(pr, v, self.start_taper(s_here, v));
            let step = (diam * end_k * self.preset.spacing).max(0.5);
            let need = step - self.acc;
            if t + need > len {
                self.acc += len - t;
                break;
            }
            t += need;
            self.acc = 0.0;
            let f = t / len;
            if let Some(d) = self.pending.take() {
                self.emit(layer, d);
            }
            let s = self.travel + t;
            let d = Dab { x: a.x + dx * f, y: a.y + dy * f, pr: a.pressure + (b.pressure - a.pressure) * f, dir, s, v, taper: self.start_taper(s, v) };
            self.emit(layer, d);
        }
        self.travel += len;
    }

    /// Caneta de pixel: o pixel sob (x, y) entra na fila; pulos viram linha de Bresenham e o pixel do meio de um degrau em
    /// L sai (linha de 1 px sem pixel dobrado).
    fn pixel_at(&mut self, x: f32, y: f32) {
        let c = (x.floor() as i32, y.floor() as i32);
        if self.pix_last == Some(c) {
            return;
        }
        let mut cells = Vec::new();
        match self.pix_last {
            Some(l) if (c.0 - l.0).abs() > 1 || (c.1 - l.1).abs() > 1 => {
                let (mut x0, mut y0) = l;
                let (dx, dy) = ((c.0 - x0).abs(), -(c.1 - y0).abs());
                let (sx, sy) = (if c.0 > x0 { 1 } else { -1 }, if c.1 > y0 { 1 } else { -1 });
                let mut err = dx + dy;
                while (x0, y0) != c {
                    let e2 = 2 * err;
                    if e2 >= dy {
                        err += dy;
                        x0 += sx;
                    }
                    if e2 <= dx {
                        err += dx;
                        y0 += sy;
                    }
                    cells.push((x0, y0));
                }
            }
            _ => cells.push(c),
        }
        self.pix_last = Some(c);
        for c in cells {
            if let [.., a, b] = self.pix[..] {
                let orth = |p: (i32, i32), q: (i32, i32)| (p.0 - q.0).abs() + (p.1 - q.1).abs() == 1;
                if orth(a, b) && orth(b, c) && (a.0 - c.0).abs() == 1 && (a.1 - c.1).abs() == 1 {
                    self.pix.pop();
                }
            }
            self.pix.push(c);
            while self.pix.len() > 2 {
                let p = self.pix.remove(0);
                self.stamp_pixel(p);
            }
        }
    }

    fn stamp_pixel(&mut self, (x, y): (i32, i32)) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let p = &self.preset;
        let da = (p.flow * self.color[3]).clamp(0.0, 1.0);
        let rgb = [self.color[0], self.color[1], self.color[2]];
        self.mark([x, y, x + 1, y + 1]);
        let d = self.buf_mut(x, y);
        let k = 1.0 - da;
        for c in 0..3 {
            d[c] = rgb[c] * da + d[c] * k;
        }
        d[3] = da + d[3] * k;
        let a = d[3];
        self.max_a = self.max_a.max(a);
    }

    /// Sorteio do aleatório de uma dinâmica: por carimbo, ou suave ao longo do caminho (`random_len`).
    fn dyn_rnd(&mut self, dy: &crate::brush_dyn::Dynamics, s: f32, k: u32) -> f32 {
        if !dy.uses_random() {
            0.0
        } else if dy.random_len > 0.0 {
            crate::brush_dyn::smooth_random(s, dy.random_len, self.noise_seed.wrapping_add(k.wrapping_mul(0x51ED)))
        } else {
            self.rng.f32()
        }
    }

    fn stamp(&mut self, layer: Option<&Image>, d: Dab) {
        let preset = self.preset.clone();
        let (cx, cy, dir) = (d.x, d.y, d.dir);
        let pr = d.pr.clamp(0.0, 1.0);
        let v = d.v;
        let color = match &self.ramp {
            Some((g, len)) => g.color_at(d.s / len),
            None => self.color,
        };
        let csp_tip = preset.antialias.is_some() || preset.edge.is_some() || preset.thickness_dyn.is_some();
        let (taper_size, taper_op) = match &preset.taper {
            Some(t) => (if t.size { d.taper } else { 1.0 }, if t.opacity { d.taper } else { 1.0 }),
            None => (1.0, 1.0),
        };
        for _ in 0..preset.count {
            let mut size = match &preset.size_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 1);
                    preset.size * dy.factor(pr, v, rnd)
                }
                None => preset.size * (preset.size_pressure_min + (1.0 - preset.size_pressure_min) * pr),
            };
            size *= taper_size;
            if preset.size_jitter > 0.0 {
                size *= 1.0 + preset.size_jitter * self.rng.signed();
            }
            let r = (size * 0.5).max(0.3);
            let (mut px, mut py) = (cx, cy);
            if preset.scatter > 0.0 {
                let perp = (dir + 90.0).to_radians();
                let off = preset.scatter * preset.size * self.rng.signed();
                px += perp.cos() * off;
                py += perp.sin() * off;
                if preset.count > 1 {
                    px += preset.scatter * preset.size * 0.5 * self.rng.signed();
                }
            }
            let mut angle = if preset.follow_direction { dir } else { 0.0 };
            if preset.angle_jitter > 0.0 {
                angle += preset.angle_jitter * self.rng.signed();
            }
            let thick = match &preset.thickness_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 2);
                    dy.factor(pr, v, rnd)
                }
                None => 1.0,
            };
            let edge = preset.edge.clone();
            let mut dab_opacity = match &preset.opacity_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 3);
                    dy.factor(pr, v, rnd)
                }
                None => preset.opacity_pressure_min + (1.0 - preset.opacity_pressure_min) * pr,
            };
            dab_opacity *= taper_op;
            let flow = match &preset.flow_dyn {
                Some(dy) => {
                    let rnd = self.dyn_rnd(dy, d.s, 4);
                    preset.flow * dy.factor(pr, v, rnd)
                }
                None => preset.flow * (preset.flow_pressure_min + (1.0 - preset.flow_pressure_min) * pr),
            };
            let mut rgb = [color[0], color[1], color[2]];
            if preset.hue_jitter > 0.0 || preset.saturation_jitter > 0.0 || preset.value_jitter > 0.0 {
                let mut hsv = rgb_to_hsv(rgb);
                hsv[0] += preset.hue_jitter * 0.5 * self.rng.signed();
                hsv[1] = (hsv[1] + preset.saturation_jitter * self.rng.signed()).clamp(0.0, 1.0);
                hsv[2] = (hsv[2] + preset.value_jitter * self.rng.signed()).clamp(0.0, 1.0);
                rgb = hsv_to_rgb(hsv);
            }
            let two = preset.two_tone.as_ref().filter(|_| matches!(preset.tip, BrushTip::Shape { .. })).map(|t| t.second(rgb));
            if preset.mix > 0.0 {
                // amostra a pegada sob o dab (camada com o traço em andamento por cima) e mistura com a que vem
                // carregada: puxa a estrutura dos pixels (listras, bordas), não uma cor média
                let pr_i = r.ceil() as i32;
                let side = (2 * pr_i + 1) as usize;
                let (sx, sy) = (px.round() as i32, py.round() as i32);
                let mut sample = vec![[0.0f32; 4]; side * side];
                for j in 0..side as i32 {
                    for i in 0..side as i32 {
                        let (xx, yy) = (sx - pr_i + i, sy - pr_i + j);
                        if xx < 0 || yy < 0 || xx >= self.w || yy >= self.h {
                            continue;
                        }
                        let l = layer.filter(|l| (xx as u32) < l.width && (yy as u32) < l.height).map(|l| l.get(xx as u32, yy as u32)).unwrap_or([0.0; 4]);
                        let b = self.buf_get(xx, yy);
                        sample[j as usize * side + i as usize] = [0, 1, 2, 3].map(|c| b[c] + l[c] * (1.0 - b[3]));
                    }
                }
                let k = preset.smudge_carry.clamp(0.0, 1.0);
                self.carry = Some(match self.carry.take() {
                    Some((cr, prev)) if cr == pr_i => (pr_i, prev.iter().zip(&sample).map(|(a, b)| [0, 1, 2, 3].map(|c| a[c] * k + b[c] * (1.0 - k))).collect()),
                    _ => (pr_i, sample),
                });
            }
            // a rampa do antisserrilhado forte passa um pouco do raio
            let pad = 1.0 + preset.antialias.map(|l| crate::brush_dyn::aa_ramp(l).0 * 0.5).unwrap_or(0.0);
            let (bx0, by0) = (((px - r - pad).floor() as i32).max(0), ((py - r - pad).floor() as i32).max(0));
            let (bx1, by1) = (((px + r + pad).ceil() as i32).min(self.w), ((py + r + pad).ceil() as i32).min(self.h));
            if bx0 < bx1 && by0 < by1 {
                self.mark([bx0, by0, bx1, by1]);
            }
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                    let mut a = if csp_tip {
                        let ek = edge.as_ref().map(|e| e.radius_at((x + self.tex_origin[0]) as f32 + 0.5, (y + self.tex_origin[1]) as f32 + 0.5)).unwrap_or(1.0);
                        tip_alpha_pen(&preset.tip, dx, dy, r, angle, preset.antialias.unwrap_or(if preset.aliased { 0 } else { 1 }), thick, ek, edge.is_some())
                            .unwrap_or_else(|| tip_alpha_tone(&preset.tip, dx, dy, r, angle, preset.aliased, two.is_some()))
                    } else {
                        tip_alpha_tone(&preset.tip, dx, dy, r, angle, preset.aliased, two.is_some())
                    };
                    if a <= 0.0 {
                        continue;
                    }
                    if let Some(t) = &preset.texture {
                        let tv = texture_value(t, (x + self.tex_origin[0]) as f32, (y + self.tex_origin[1]) as f32);
                        a = match t.mode {
                            TextureMode::Multiply => a * (1.0 - t.depth + t.depth * tv),
                            TextureMode::Subtract => (a - (1.0 - tv) * t.depth).max(0.0),
                        };
                    }
                    let da = (a * flow * dab_opacity * color[3]).clamp(0.0, 1.0);
                    if da <= 0.0 {
                        continue;
                    }
                    let mut src = [rgb[0], rgb[1], rgb[2], 1.0];
                    if let (Some(c2), Some((shape, u, v))) = (two, shape_uv(&preset.tip, dx, dy, r, angle)) {
                        let k = shape_tone(shape, u, v, 1.0 / r.max(0.25));
                        for c in 0..3 {
                            src[c] += (c2[c] - src[c]) * k;
                        }
                    }
                    if let Some((cr, cp)) = &self.carry {
                        let side = (2 * cr + 1) as usize;
                        let (ci, cj) = (x - (px.round() as i32 - cr), y - (py.round() as i32 - cr));
                        if ci >= 0 && cj >= 0 && (ci as usize) < side && (cj as usize) < side {
                            let cv = cp[cj as usize * side + ci as usize];
                            for c in 0..4 {
                                src[c] = src[c] * (1.0 - preset.mix) + cv[c] * preset.mix;
                            }
                        }
                    }
                    let max_ink = preset.max_ink;
                    let d = self.buf_mut(x, y);
                    if max_ink {
                        if da > d[3] {
                            for c in 0..4 {
                                d[c] = src[c] * da;
                            }
                        }
                    } else {
                        let k = 1.0 - da;
                        for c in 0..4 {
                            d[c] = src[c] * da + d[c] * k;
                        }
                    }
                    let a = d[3];
                    self.max_a = self.max_a.max(a);
                }
            }
        }
    }

    /// Caixa [x0, y0, x1, y1) dos tiles que o traço tocou (coordenadas locais), presa à camada.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        if self.tiles.is_empty() {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for (tx, ty) in self.tiles.keys() {
            x0 = x0.min(tx * STS);
            y0 = y0.min(ty * STS);
            x1 = x1.max((tx + 1) * STS);
            y1 = y1.max((ty + 1) * STS);
        }
        let b = [x0.max(0), y0.max(0), x1.min(self.w), y1.min(self.h)];
        (b[0] < b[2] && b[1] < b[3]).then_some(b)
    }

    /// Compõe o traço em `img` (um recorte da camada que começa em `img_origin`, coordenadas locais) com a opacidade
    /// e o modo do pincel, depois do pincel duplo e das bordas molhadas. Devolve a caixa composta (local).
    pub fn composite(&self, img: &mut Image, img_origin: [i32; 2]) -> Option<PaintResult> {
        let b = self.bounds()?;
        self.composite_in(img, img_origin, b, None)
    }

    /// Como [`composite`](Self::composite), só no retângulo `rect` [x0, y0, x1, y1) (prévia ao vivo: só o que mudou).
    /// O nível da borda molhada é o do traço inteiro (sem pincel duplo, exato; com ele, aproximado pelo alfa antes
    /// da máscara da segunda ponta).
    pub fn composite_rect(&self, img: &mut Image, img_origin: [i32; 2], rect: [i32; 4]) -> Option<PaintResult> {
        self.composite_in(img, img_origin, rect, Some(self.max_a))
    }

    fn composite_in(&self, img: &mut Image, img_origin: [i32; 2], area: [i32; 4], level: Option<f32>) -> Option<PaintResult> {
        let [bx0, by0, bx1, by1] = area;
        if self.tiles.is_empty() || bx0 >= bx1 || by0 >= by1 {
            return None;
        }
        let preset = &self.preset;
        // borda molhada desfoca a máscara até `rad` px: a região densa ganha essa margem (presa à camada)
        let rad = (preset.size * 0.12).clamp(1.0, 12.0) as i32;
        let m = if preset.wet_edges > 0.0 { rad } else { 0 };
        let (x0, y0, x1, y1) = ((bx0 - m).max(0), (by0 - m).max(0), (bx1 + m).min(self.w), (by1 + m).min(self.h));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
        let mut buf = vec![[0.0f32; 4]; bw * bh];
        for ((tx, ty), t) in &self.tiles {
            for j in 0..STS {
                let y = ty * STS + j;
                if y < y0 || y >= y1 {
                    continue;
                }
                for i in 0..STS {
                    let x = tx * STS + i;
                    if x < x0 || x >= x1 {
                        continue;
                    }
                    buf[(y - y0) as usize * bw + (x - x0) as usize] = t[(j * STS + i) as usize];
                }
            }
        }
        if let Some(db) = &self.dual {
            for y in y0..y1 {
                for x in x0..x1 {
                    let mval = db.get(&(x.div_euclid(STS), y.div_euclid(STS))).map(|t| t[(y.rem_euclid(STS) * STS + x.rem_euclid(STS)) as usize]).unwrap_or(0.0);
                    for c in buf[(y - y0) as usize * bw + (x - x0) as usize].iter_mut() {
                        *c *= mval;
                    }
                }
            }
        }
        // bordas molhadas: a poça tem borda definida e o pigmento se acumula na borda de FORA (como a aquarela seca).
        if preset.wet_edges > 0.0 {
            let level = level.unwrap_or_else(|| buf.iter().map(|p| p[3]).fold(0.0f32, f32::max));
            let smooth = |a: f32| {
                let t = ((a / level.max(1e-6) - 0.08) / 0.30).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let mask: Vec<f32> = buf.iter().map(|p| smooth(p[3])).collect();
            let blurred = box_blur(&mask, bw, bh, rad);
            for (i, p) in buf.iter_mut().enumerate() {
                if p[3] <= 0.0 {
                    continue;
                }
                let edge = ((mask[i] - blurred[i]).max(0.0) * 3.0).min(1.0);
                let factor = (1.0 - preset.wet_edges * 0.5) + preset.wet_edges * 0.8 * edge;
                let na = (mask[i] * level * factor).min(1.0);
                let s = if p[3] > 0.0 { na / p[3] } else { 0.0 };
                for c in p.iter_mut() {
                    *c *= s;
                }
            }
        }
        let mode = match preset.mode {
            BrushMode::Normal => BlendMode::Normal,
            BrushMode::Behind => BlendMode::Behind,
            BrushMode::Multiply => BlendMode::Multiply,
            BrushMode::Screen => BlendMode::Screen,
            BrushMode::Overlay => BlendMode::Overlay,
            BrushMode::Add => BlendMode::Add,
            BrushMode::Erase => BlendMode::Normal,
        };
        let (iw, ih) = (img.width as i32, img.height as i32);
        let (wx0, wy0, wx1, wy1) = if level.is_some() { (bx0.max(x0), by0.max(y0), bx1.min(x1), by1.min(y1)) } else { (x0, y0, x1, y1) };
        for y in wy0..wy1 {
            let iy = y - img_origin[1];
            if iy < 0 || iy >= ih {
                continue;
            }
            for x in wx0..wx1 {
                let ix = x - img_origin[0];
                if ix < 0 || ix >= iw {
                    continue;
                }
                let s = buf[(y - y0) as usize * bw + (x - x0) as usize];
                if s[3] <= 0.0 {
                    continue;
                }
                let i = img.idx(ix as u32, iy as u32);
                if preset.mode == BrushMode::Erase {
                    let k = 1.0 - s[3] * preset.opacity;
                    for c in img.data[i].iter_mut() {
                        *c *= k;
                    }
                } else {
                    blend_pixel(&mut img.data[i], s, mode, preset.opacity);
                }
            }
        }
        Some(PaintResult { bounds: [wx0, wy0, wx1, wy1] })
    }
}

fn box_blur(src: &[f32], w: usize, h: usize, r: i32) -> Vec<f32> {
    let mut tmp = vec![0.0f32; w * h];
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (a, b) = ((x as i32 - r).max(0) as usize, ((x as i32 + r) as usize).min(w - 1));
            let s: f32 = src[y * w + a..=y * w + b].iter().sum();
            tmp[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    for x in 0..w {
        for y in 0..h {
            let (a, b) = ((y as i32 - r).max(0) as usize, ((y as i32 + r) as usize).min(h - 1));
            let mut s = 0.0;
            for yy in a..=b {
                s += tmp[yy * w + x];
            }
            out[y * w + x] = s / (b - a + 1) as f32;
        }
    }
    out
}

/// Degradê linear/radial/angular/diamante entre dois pontos do canvas, paradas sRGB (t, cor não pré-multiplicada),
/// preso nas pontas: a ferramenta Degradê de sempre, agora pelo avaliador único de `aiai_core::paint`.
pub fn gradient_fill(layer: &mut Image, kind: GradientKind, p0: [f32; 2], p1: [f32; 2], stops: &[(f32, [f32; 4])], mode: BlendMode, opacity: f32) {
    if stops.is_empty() {
        return;
    }
    let g = aiai_core::ResolvedGradient::simple(stops, kind.shape(), p0, p1);
    let bbox = [0.0, 0.0, layer.width as f32, layer.height as f32];
    gradient_fill_resolved(layer, &g, bbox, mode, opacity);
}

/// Pinta a camada inteira com um degradê resolvido (geometria relativa a `bbox` ou absoluta).
pub fn gradient_fill_resolved(layer: &mut Image, g: &aiai_core::ResolvedGradient, bbox: [f32; 4], mode: BlendMode, opacity: f32) {
    if g.stops.is_empty() {
        return;
    }
    let m = g.mapper(bbox);
    let wdt = layer.width as usize;
    for (i, px) in layer.data.iter_mut().enumerate() {
        let (x, y) = ((i % wdt) as f32 + 0.5, (i / wdt) as f32 + 0.5);
        let c = g.sample(&m, [x, y]);
        blend_pixel(px, [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]], mode, opacity);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    Linear,
    Radial,
    /// Cônico em volta de `from`, começando na direção de `to`.
    Angular,
    /// Losango em volta de `from` (raio = distância até `to`).
    Diamond,
}

impl GradientKind {
    pub fn shape(self) -> aiai_core::GradientShape {
        match self {
            GradientKind::Linear => aiai_core::GradientShape::Linear,
            GradientKind::Radial => aiai_core::GradientShape::Radial,
            GradientKind::Angular => aiai_core::GradientShape::Angular,
            GradientKind::Diamond => aiai_core::GradientShape::Diamond,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> Vec<BrushPoint> {
        (0..n).map(|i| BrushPoint { x: 20.0 + i as f32 * 4.0, y: 50.0, pressure: 1.0 }).collect()
    }

    #[test]
    fn deterministic_and_respects_opacity_cap() {
        let mut p = BrushPreset::round("t", 20.0, 0.9);
        p.opacity = 0.5;
        p.flow = 0.3;
        let mut a = Image::new(200, 100);
        let mut b = Image::new(200, 100);
        paint_stroke(&mut a, &p, &line(40), [1.0, 0.0, 0.0, 1.0], 7).unwrap();
        paint_stroke(&mut b, &p, &line(40), [1.0, 0.0, 0.0, 1.0], 7).unwrap();
        assert_eq!(a.content_hash(), b.content_hash());
        let max_a = a.data.iter().map(|q| q[3]).fold(0.0, f32::max);
        assert!(max_a <= 0.5 + 1e-4 && max_a > 0.45, "opacidade do traço limita o acúmulo: {max_a}");
    }

    #[test]
    fn eraser_removes_and_texture_varies_alpha() {
        let mut img = Image::filled(200, 100, [0.2, 0.4, 0.6, 1.0]);
        let mut e = BrushPreset::round("e", 20.0, 1.0);
        e.mode = BrushMode::Erase;
        paint_stroke(&mut img, &e, &line(30), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        assert!(img.get(60, 50)[3] < 0.01);
        let mut t = BrushPreset::round("t", 30.0, 1.0);
        t.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 2.0, depth: 1.0, mode: TextureMode::Multiply, seed: 1, invert: false, image: None, image_mask: None });
        let mut img2 = Image::new(200, 100);
        paint_stroke(&mut img2, &t, &line(30), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        let vals: Vec<f32> = (40..80).map(|x| img2.get(x, 50)[3]).collect();
        let mn = vals.iter().cloned().fold(1.0, f32::min);
        let mx = vals.iter().cloned().fold(0.0, f32::max);
        assert!(mx - mn > 0.2, "textura modula o alfa: {mn}..{mx}");
    }

    #[test]
    fn eraser_is_not_a_factory_preset_but_still_a_valid_eraser() {
        assert!(BrushPreset::defaults().iter().all(|p| p.name != "Borracha" && p.mode != BrushMode::Erase && p.category != "borracha"));
        let e = BrushPreset::eraser();
        e.validate().unwrap();
        assert_eq!((e.name.as_str(), e.mode), ("Borracha", BrushMode::Erase));
        let mut img = Image::filled(120, 60, [0.2, 0.4, 0.9, 1.0]);
        let before = img.content_hash();
        paint_stroke(&mut img, &e, &line(30), [0.0, 0.0, 0.0, 1.0], 1);
        assert_ne!(img.content_hash(), before, "apaga");
    }

    #[test]
    fn all_default_presets_paint_something() {
        for p in BrushPreset::defaults() {
            p.validate().unwrap();
            let mut img = Image::filled(220, 100, [0.9, 0.9, 0.9, 1.0]);
            let before = img.content_hash();
            paint_stroke(&mut img, &p, &line(40), [0.2, 0.3, 0.8, 1.0], 3);
            assert_ne!(img.content_hash(), before, "preset {} não mudou nada", p.name);
        }
    }

    #[test]
    fn watercolor_pigment_gathers_at_the_outer_edge() {
        let wc = BrushPreset::defaults().into_iter().find(|p| p.name == "Aquarela").unwrap();
        let mut plain = wc.clone();
        plain.texture = None;
        let mut img = Image::new(240, 120);
        paint_stroke(&mut img, &plain, &line(40), [0.2, 0.3, 0.8, 1.0], 3).unwrap();
        // corte vertical no meio do traço (y = 50 é o eixo; raio 25)
        let col: Vec<f32> = (20..80).map(|y| img.get(100, y)[3]).collect();
        let center = img.get(100, 50)[3];
        let peak = col.iter().cloned().fold(0.0, f32::max);
        let peak_y = 20 + col.iter().position(|&v| v == peak).unwrap() as i32;
        assert!(peak > center * 1.15, "borda mais carregada que o centro: pico {peak} x centro {center}");
        assert!((peak_y - 50).abs() >= 15, "o pico fica perto da borda de fora (raio 25), não no joelho de dentro: y {peak_y}");
        assert!(img.get(100, 50 + 30)[3] < 0.02, "fora da poça não há halo");
    }

    #[test]
    fn smudge_drags_pixel_structure_not_an_average_color() {
        // listras horizontais vermelho/azul de 6 px à esquerda; o borrador puxa da esquerda para a direita
        let mut img = Image::new(240, 120);
        for y in 0..120u32 {
            let c = if (y / 6) % 2 == 0 { [1.0, 0.0, 0.0, 1.0] } else { [0.0, 0.0, 1.0, 1.0] };
            for x in 0..100u32 {
                let i = img.idx(x, y);
                img.data[i] = c;
            }
        }
        let sm = BrushPreset::defaults().into_iter().find(|p| p.name == "Borrador").unwrap();
        let path: Vec<BrushPoint> = (0..40).map(|i| BrushPoint { x: 80.0 + i as f32 * 1.5, y: 60.0, pressure: 1.0 }).collect();
        paint_stroke(&mut img, &sm, &path, [0.5, 0.5, 0.5, 1.0], 1).unwrap();
        // logo depois da borda da tinta, as linhas continuam distintas (uma média daria roxo igual nas duas)
        let (x, red_row, blue_row) = (106, 60, 66);
        let (a, b) = (img.get(x, red_row), img.get(x, blue_row));
        assert!(a[3] > 0.2 && b[3] > 0.2, "a tinta foi arrastada para fora: {a:?} {b:?}");
        let ra = a[0] / a[3].max(1e-6);
        let rb = b[0] / b[3].max(1e-6);
        assert!(ra - rb > 0.3, "linha vermelha continua mais vermelha que a azul: {ra} x {rb}");
    }

    /// Prancha de todos os presets (evidência visual): `AIAI_EVIDENCE_DIR=... cargo test -p aiai-paint brush_board -- --ignored`.
    #[test]
    #[ignore]
    fn brush_board() {
        let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") else { return };
        let presets = BrushPreset::defaults();
        let (cw, rh) = (560u32, 70u32);
        let rows = presets.len().div_ceil(2) as u32;
        let mut img = Image::filled(cw * 2, rh * rows, [0.97, 0.96, 0.93, 1.0]);
        // listras para o borrador ter o que arrastar
        for (k, p) in presets.iter().enumerate() {
            let (ox, oy) = ((k as u32 % 2) * cw, (k as u32 / 2) * rh);
            if p.mix > 0.5 {
                for y in oy + 10..oy + rh - 10 {
                    let c = if ((y - oy) / 6) % 2 == 0 { [0.85, 0.2, 0.2, 1.0] } else { [0.2, 0.3, 0.8, 1.0] };
                    for x in ox + 20..ox + 200 {
                        let i = img.idx(x, y);
                        img.data[i] = c;
                    }
                }
            }
            let path: Vec<BrushPoint> = (0..120)
                .map(|i| {
                    let t = i as f32 / 119.0;
                    BrushPoint { x: ox as f32 + 40.0 + t * 480.0, y: oy as f32 + rh as f32 / 2.0 + 14.0 * (t * 6.0).sin(), pressure: 0.3 + 0.7 * (t * 3.14).sin() }
                })
                .collect();
            paint_stroke(&mut img, p, &path, [0.15, 0.35, 0.6, 1.0], 9);
        }
        img.save_png(&std::path::Path::new(&dir).join("pinceis_prancha.png")).unwrap();
    }

    #[test]
    fn color_ramp_runs_along_the_stroke_length_and_feeds_in_chunks_the_same() {
        let mut p = BrushPreset::round("r", 10.0, 1.0);
        p.color_ramp = Some(ColorRamp { gradient: aiai_core::Gradient::from_colors(&[[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]], 0.0), length: 100.0 });
        let path: Vec<BrushPoint> = (0..=40).map(|i| BrushPoint { x: 20.0 + i as f32 * 4.0, y: 50.0, pressure: 1.0 }).collect();
        let mut a = Image::new(220, 100);
        paint_stroke(&mut a, &p, &path, [0.0, 1.0, 0.0, 1.0], 3).unwrap();
        let un = |q: [f32; 4]| [q[0] / q[3].max(1e-6), q[1] / q[3].max(1e-6), q[2] / q[3].max(1e-6)];
        let start = un(a.get(21, 50));
        let mid = un(a.get(70, 50));
        let end = un(a.get(170, 50));
        assert!(start[0] > 0.85 && start[2] < 0.15, "começo vermelho: {start:?}");
        assert!(mid[0] > 0.3 && mid[2] > 0.3, "meio misturado: {mid:?}");
        assert!(end[2] > 0.95 && end[1] < 0.01, "depois de 100 px fica azul (pad), e a cor do pincel não entra: {end:?}");
        // aos pedaços: o mesmo resultado
        let mut sb = StrokeBuffer::new(&p, [0.0, 1.0, 0.0, 1.0], 3, 220, 100, [0, 0]);
        for chunk in path.chunks(7) {
            sb.add(None, chunk);
        }
        sb.finish(None);
        let mut b = Image::new(220, 100);
        sb.composite(&mut b, [0, 0]);
        let d = a.data.iter().zip(&b.data).map(|(x, y)| (0..4).map(|k| (x[k] - y[k]).abs()).fold(0.0, f32::max)).fold(0.0, f32::max);
        assert!(d < 1e-4, "pedaços = inteiro: {d}");
    }

    #[test]
    fn gradient_fill_kinds_cover_the_layer() {
        let stops = [(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0, 1.0, 1.0, 1.0])];
        for kind in [GradientKind::Linear, GradientKind::Radial, GradientKind::Angular, GradientKind::Diamond] {
            let mut img = Image::new(64, 64);
            gradient_fill(&mut img, kind, [32.0, 32.0], [64.0, 32.0], &stops, BlendMode::Normal, 1.0);
            assert!(img.data.iter().all(|q| q[3] > 0.999), "{kind:?} cobre tudo");
            let vals: Vec<f32> = img.data.iter().map(|q| q[0]).collect();
            let (mn, mx) = (vals.iter().cloned().fold(1.0, f32::min), vals.iter().cloned().fold(0.0, f32::max));
            assert!(mx - mn > 0.5, "{kind:?} varia: {mn}..{mx}");
        }
    }
}
