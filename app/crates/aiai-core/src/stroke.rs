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
    /// Canto: a curva quebra neste ponto (entra e sai em linha, sem arredondar). Só vale para Spline/Freehand. Na
    /// primeira ou na última ponta de um traço aberto: ponta reta (não afina), como a ponta soldada em outra linha.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub corner: bool,
    /// Multiplicador da largura neste ponto (1 = a do traço), interpolado ao longo da curva como a pressão; vale
    /// para qualquer ponta (a pressão não muda a largura do marcador).
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub width: f32,
    /// Id estável do ponto dentro do traço (0 = ainda sem id: vale índice+1 até a primeira inserção/remoção, quando
    /// os ids são gravados; ver [`Stroke::point_id`] e [`Stroke::materialize_point_ids`]).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub id: u32,
    /// Alça (tangente) explícita do ponto em px do cel: a curva sai na direção `+handle` e chega por `-handle`; o
    /// comprimento abre (mais redondo) ou fecha (mais justo) a curva ali. `None` = automática (spline). Só para
    /// Spline/Freehand e quando o ponto não é canto.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<[f32; 2]>,
}

fn one() -> f32 {
    1.0
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

fn is_one(v: &f32) -> bool {
    *v == 1.0
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
    /// Lápis vetorial (largura pela pressão, sem afinar).
    Pencil,
    /// Pena escolar (school pen): fina e quase uniforme, pontas curtas.
    School,
    /// Pincel japonês (fude): muita variação com a pressão e pontas longas e finas.
    Fude,
    /// Ponta quadrada: largura constante e pontas cortadas retas (sem tampa redonda).
    Square,
    /// Lápis de genga: como o Lápis, com grão de papel no traço.
    Genga,
}

impl VectorTip {
    pub const ALL: [VectorTip; 8] = [VectorTip::GPen, VectorTip::MaruPen, VectorTip::School, VectorTip::Fude, VectorTip::Marker, VectorTip::Square, VectorTip::Pencil, VectorTip::Genga];

    /// Ponta de lápis (fica no grupo Lápis da prateleira).
    pub fn is_pencil(self) -> bool {
        matches!(self, VectorTip::Pencil | VectorTip::Genga)
    }
}

/// Unidade do comprimento da entrada e da saída.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaperUnit {
    /// Pixels de caminho.
    #[default]
    Px,
    /// Porcentagem do comprimento do traço (0..100).
    Percent,
}

/// Entrada e saída de um traço vetorial (afinamento das pontas), como no Clip Studio: comprimento em px ou em % do
/// traço e, se `by_speed`, proporcional à velocidade em cada ponta (traço lento quase não afina; pede o tempo `t`
/// dos pontos — sem ele conta como rápido). PRESENTE no traço = substitui o afinamento automático da ponta (`tip`):
/// `start = end = 0` desliga o afinamento. Ausente = a regra antiga da ponta (traços gravados antes continuam iguais).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct StrokeTaper {
    /// Comprimento da entrada (px ou %); 0 = sem.
    #[serde(default)]
    pub start: f32,
    /// Comprimento da saída (px ou %); 0 = sem.
    #[serde(default)]
    pub end: f32,
    #[serde(default)]
    pub unit: TaperUnit,
    /// Espessura na ponta (fração 0..1 da largura).
    #[serde(default)]
    pub min: f32,
    #[serde(default)]
    pub by_speed: bool,
}

impl StrokeTaper {
    /// Sem afinamento nenhum (nem o automático da ponta).
    pub const NONE: StrokeTaper = StrokeTaper { start: 0.0, end: 0.0, unit: TaperUnit::Px, min: 0.0, by_speed: false };

    /// Velocidade (px/s) que conta como 100% no "pela velocidade" (a mesma do pincel raster).
    pub const VELOCITY_FULL: f32 = 2500.0;

    pub fn validate(&self) -> Result<(), String> {
        let max = if self.unit == TaperUnit::Percent { 100.0 } else { 5000.0 };
        if !(self.start.is_finite() && self.end.is_finite() && (0.0..=max).contains(&self.start) && (0.0..=max).contains(&self.end)) {
            return Err(format!("taper: entrada e saída entre 0 e {max}"));
        }
        if !(0.0..=1.0).contains(&self.min) {
            return Err(format!("taper.min fora de 0..1: {}", self.min));
        }
        Ok(())
    }
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
    /// Entrada e saída configuradas (substitui o afinamento automático da ponta); ausente = regra da ponta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taper: Option<StrokeTaper>,
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
    /// Grupo de intercalação (auditoria 1.5a): traços com grupo só casam com traços do mesmo grupo entre duas chaves;
    /// traço a mais num grupo nasce do par do grupo (1 para N). Vazio = casa com qualquer um.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
}

fn is_empty_profile(p: &WidthProfile) -> bool {
    p.0.is_empty()
}

impl Stroke {
    /// Id estável do ponto `i` (o gravado, ou índice+1 enquanto o traço nunca teve ponto inserido/apagado).
    pub fn point_id(&self, i: usize) -> u32 {
        match self.points.get(i) {
            Some(q) if q.id != 0 => q.id,
            _ => i as u32 + 1,
        }
    }

    /// Índice do ponto com este id estável.
    pub fn point_index(&self, id: u32) -> Option<usize> {
        (0..self.points.len()).find(|&i| self.point_id(i) == id)
    }

    /// Grava os ids antes de inserir/apagar pontos (assim os ids lidos antes continuam valendo): sem nenhum gravado,
    /// cada ponto recebe índice+1; os que faltarem depois recebem o maior + 1, em ordem.
    pub fn materialize_point_ids(&mut self) {
        if self.points.iter().all(|q| q.id == 0) {
            for (i, q) in self.points.iter_mut().enumerate() {
                q.id = i as u32 + 1;
            }
            return;
        }
        let mut next = self.points.iter().map(|q| q.id).max().unwrap_or(0) + 1;
        for q in self.points.iter_mut().filter(|q| q.id == 0) {
            q.id = next;
            next += 1;
        }
    }

    /// Id para um ponto novo (depois de `materialize_point_ids`).
    pub fn next_point_id(&self) -> u32 {
        self.points.iter().map(|q| q.id).max().unwrap_or(self.points.len() as u32) + 1
    }

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
        if let Some(tp) = &self.taper {
            tp.validate()?;
        }
        for p in &self.points {
            if !(p.x.is_finite() && p.y.is_finite() && (0.0..=1.0).contains(&p.pressure)) {
                return Err("ponto inválido (coordenada não finita ou pressão fora de 0..1)".into());
            }
            if p.handle.is_some_and(|h| !(h[0].is_finite() && h[1].is_finite())) {
                return Err("alça do ponto não finita".into());
            }
            if !(p.width.is_finite() && (0.0..=20.0).contains(&p.width)) {
                return Err(format!("largura do ponto fora de 0..20 (multiplicador): {}", p.width));
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

#[cfg(test)]
mod point_id_tests {
    use super::*;

    fn pt(x: f32) -> StrokePoint {
        StrokePoint { x, y: 0.0, pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
    }

    #[test]
    fn ids_read_before_an_insert_still_point_to_the_same_points() {
        let mut s = Stroke { taper: None, id: Id::from("s"), points: vec![pt(0.0), pt(10.0), pt(20.0)], geometry: StrokeGeometry::Spline, tension: 0.0, tip: VectorTip::Marker, width: 2.0, profile: Default::default(), color: ColorRef::default(), opacity: 1.0, closed: false, aliased: false, color_trace: false, group: String::new() };
        assert_eq!((s.point_id(0), s.point_id(2)), (1, 3), "sem ids gravados: índice+1");
        s.materialize_point_ids();
        let mut q = pt(5.0);
        q.id = s.next_point_id();
        s.points.insert(1, q);
        assert_eq!(s.point_index(2), Some(2), "o ponto 2 (x=10) continua com o id 2");
        assert_eq!(s.points[s.point_index(4).unwrap()].x, 5.0, "o novo ganhou o id 4");
        let json = serde_json::to_string(&pt(1.0)).unwrap();
        assert!(!json.contains("id") && !json.contains("handle"), "sem id/alça não grava nada: {json}");
    }
}
