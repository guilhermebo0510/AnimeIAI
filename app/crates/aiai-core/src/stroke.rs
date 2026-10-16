//! Dados de traço vetorial (a geometria e a rasterização moram em `aiai-paint`).

use crate::color::ColorRef;
use crate::ids::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Amostra de entrada (caneta, mouse ou comando). Coordenadas em pixels do quadro do corte (origem no canto superior esquerdo).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..=1
    #[serde(default = "one")]
    pub pressure: f32,
    /// Inclinação da caneta em graus (opcional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tilt: Option<[f32; 2]>,
    /// Tempo relativo ao início do traço, em segundos (opcional; replay e dinâmica por velocidade).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub t: Option<f32>,
}

fn one() -> f32 {
    1.0
}

fn empty_id() -> Id {
    Id(String::new())
}

/// Como os pontos viram curva.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrokeGeometry {
    /// Amostras livres (mão/caneta); suavizadas e ajustadas por Bézier no motor.
    #[default]
    Freehand,
    /// Pontos de passagem: spline Catmull-Rom centrípeta (o modo preferido do agente).
    Spline,
    /// Polilinha reta entre os pontos.
    Polyline,
    /// Bézier cúbica explícita: [p0, c1, c2, p1, c1, c2, p2, ...].
    Bezier,
}

/// Ponta do pincel vetorial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum VectorTip {
    /// Largura segue a pressão e afina nas pontas (linha principal de anime).
    #[default]
    GPen,
    /// Fina e uniforme, pouca variação.
    MaruPen,
    /// Largura constante.
    Marker,
    /// Lápis vetorial (textura de grafite; P1).
    Pencil,
}

/// Perfil de largura ao longo do traço: pares (t em 0..=1, multiplicador da largura base).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct WidthProfile(pub Vec<[f32; 2]>);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Stroke {
    /// Vazio (ou omitido) = gerado pela sessão.
    #[serde(default = "empty_id")]
    pub id: Id,
    pub points: Vec<StrokePoint>,
    #[serde(default)]
    pub geometry: StrokeGeometry,
    /// Tensão da spline (0 = Catmull-Rom padrão).
    #[serde(default)]
    pub tension: f32,
    #[serde(default)]
    pub tip: VectorTip,
    /// Largura base em pixels.
    pub width: f32,
    /// Perfil opcional; vazio = só pressão e regra da ponta.
    #[serde(default, skip_serializing_if = "is_empty_profile")]
    pub profile: WidthProfile,
    #[serde(default)]
    pub color: ColorRef,
    #[serde(default = "one")]
    pub opacity: f32,
    #[serde(default)]
    pub closed: bool,
    /// Sem antisserrilhado (linha de cel, compatível com balde e seleção por cor).
    #[serde(default)]
    pub aliased: bool,
    /// Linha de cor (色トレス): delimita sombra/realce; pode sumir no render final conforme ajuste da camada.
    #[serde(default)]
    pub color_trace: bool,
}

fn is_empty_profile(p: &WidthProfile) -> bool {
    p.0.is_empty()
}

impl Stroke {
    pub fn validate(&self) -> Result<(), String> {
        if self.points.is_empty() {
            return Err("traço sem pontos".into());
        }
        if !(self.width.is_finite() && self.width > 0.0 && self.width <= 2000.0) {
            return Err(format!("largura fora do intervalo: {}", self.width));
        }
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(format!("opacidade fora de 0..1: {}", self.opacity));
        }
        for p in &self.points {
            if !(p.x.is_finite() && p.y.is_finite() && (0.0..=1.0).contains(&p.pressure)) {
                return Err("ponto inválido (coordenada não finita ou pressão fora de 0..1)".into());
            }
        }
        for [t, m] in &self.profile.0 {
            if !((0.0..=1.0).contains(t) && m.is_finite() && *m >= 0.0) {
                return Err("perfil de largura inválido".into());
            }
        }
        if self.geometry == StrokeGeometry::Bezier && (self.points.len() < 4 || (self.points.len() - 1) % 3 != 0) {
            return Err("Bézier precisa de 3n+1 pontos".into());
        }
        Ok(())
    }
}
