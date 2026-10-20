//! Tinta: cor chapada (`ColorRef`) OU degradê (`Gradient`) OU padrão de imagem, como tipo de cor do programa.
//!
//! O degradê tem paradas (posição 0..1, cor direta ou de paleta, opacidade), forma (linear, radial, angular/cônico,
//! diamante), ângulo/centro/escala relativos à caixa do que é pintado (ou dois pontos absolutos do canvas, como na
//! ferramenta Degradê), repetição (prender, repetir, espelhar) e espaço de interpolação:
//! - `srgb`: mistura os componentes sRGB sem pré-multiplicar (o comportamento de sempre do `gradient_fill`; os documentos
//!   antigos saem idênticos);
//! - `linear`: mistura em luz linear, pré-multiplicada pelo alfa (sem a faixa escura no meio de vermelho→verde);
//! - `oklab`: mistura no OKLab, pré-multiplicada (passagens perceptualmente uniformes, como o CSS Color 4).
//!
//! A avaliação (`ResolvedGradient`) mora aqui, sem dependência de imagem: o motor de pintura, o render do texto, as
//! regiões de preenchimento e o pincel usam o mesmo código.
//!
//! Convenção de ângulo: graus no sentido horário a partir de "da esquerda para a direita" (o y do canvas desce):
//! 0 = esquerda→direita, 90 = de cima para baixo, 180 = direita→esquerda, 270 = de baixo para cima.

use crate::blob::BlobHash;
use crate::color::{ColorRef, Rgba};
use crate::ids::Id;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Versão do esquema de `Gradient`.
pub const GRADIENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientShape {
    #[default]
    Linear,
    Radial,
    /// Cônico: a posição é o ângulo em volta do centro.
    Angular,
    /// Losango: |x| + |y| no referencial girado.
    Diamond,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientExtend {
    /// Fora de 0..1 fica a cor da ponta.
    #[default]
    Pad,
    Repeat,
    /// Vai e volta (espelhado).
    Reflect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum GradientSpace {
    #[default]
    Srgb,
    Linear,
    Oklab,
}

fn one() -> f32 {
    1.0
}

fn no_id() -> Id {
    Id(String::new())
}

/// Uma parada do degradê.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GradientStop {
    /// Posição 0..1.
    pub pos: f32,
    pub color: ColorRef,
    /// Multiplica o alfa da cor (0..1).
    #[serde(default = "one")]
    pub opacity: f32,
}

impl GradientStop {
    pub fn direct(pos: f32, rgba: [f32; 4]) -> GradientStop {
        GradientStop { pos, color: ColorRef::Direct { rgba: Rgba(rgba) }, opacity: 1.0 }
    }
}

/// Degradê. Todos os campos têm padrão (JSON parcial vale).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Gradient {
    pub v: u32,
    pub shape: GradientShape,
    pub stops: Vec<GradientStop>,
    /// Graus (ver a convenção no cabeçalho do módulo).
    pub angle: f32,
    /// Centro relativo à caixa pintada (0..1; [0.5, 0.5] = meio).
    pub center: [f32; 2],
    /// 1 = o degradê cobre a caixa; 0,5 = metade (com `repeat` repete duas vezes).
    pub scale: f32,
    pub extend: GradientExtend,
    pub space: GradientSpace,
    /// Dois pontos do canvas [de, até] em vez da caixa (ferramenta Degradê): linear vai de um ao outro, radial/diamante
    /// usa a distância como raio, angular começa na direção de→até. Sempre serializado (`null` = caixa), para um JSON
    /// Merge Patch conseguir apagar.
    pub absolute: Option<[[f32; 2]; 2]>,
}

impl Default for Gradient {
    fn default() -> Self {
        Gradient {
            v: GRADIENT_VERSION,
            shape: GradientShape::Linear,
            stops: vec![GradientStop::direct(0.0, [0.0, 0.0, 0.0, 1.0]), GradientStop::direct(1.0, [1.0, 1.0, 1.0, 1.0])],
            angle: 0.0,
            center: [0.5, 0.5],
            scale: 1.0,
            extend: GradientExtend::Pad,
            space: GradientSpace::Srgb,
            absolute: None,
        }
    }
}

impl Gradient {
    /// Degradê linear de duas ou mais cores diretas, igualmente espaçadas.
    pub fn from_colors(colors: &[[f32; 4]], angle: f32) -> Gradient {
        let n = colors.len().max(1);
        let stops = colors.iter().enumerate().map(|(i, c)| GradientStop::direct(if n == 1 { 0.0 } else { i as f32 / (n - 1) as f32 }, *c)).collect();
        Gradient { stops, angle, ..Default::default() }
    }

    /// Paradas `[(pos, "#rrggbb[aa]")]` (formato da ferramenta `aiai_gradient`).
    pub fn from_hex_stops(stops: &[(f32, String)]) -> Result<Gradient, String> {
        let stops = stops.iter().map(|(t, h)| Rgba::from_hex(h).map(|c| GradientStop::direct(*t, c.0)).ok_or_else(|| format!("cor inválida {h}"))).collect::<Result<Vec<_>, _>>()?;
        Ok(Gradient { stops, ..Default::default() })
    }

    /// Valores dentro dos limites; paradas ordenadas; sem paradas, preto→branco.
    pub fn sanitized(mut self) -> Gradient {
        let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
        self.angle = fin(self.angle, 0.0);
        self.center = [fin(self.center[0], 0.5), fin(self.center[1], 0.5)];
        self.scale = fin(self.scale, 1.0).clamp(0.001, 1000.0);
        for s in &mut self.stops {
            s.pos = fin(s.pos, 0.0).clamp(0.0, 1.0);
            s.opacity = fin(s.opacity, 1.0).clamp(0.0, 1.0);
        }
        self.stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
        if self.stops.is_empty() {
            self.stops = Gradient::default().stops;
        }
        self
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.stops.is_empty() {
            return Err("degradê sem paradas".into());
        }
        if self.stops.len() > 256 {
            return Err(format!("degradê com {} paradas (máximo 256)", self.stops.len()));
        }
        for s in &self.stops {
            if !s.pos.is_finite() || !(0.0..=1.0).contains(&s.pos) {
                return Err(format!("parada fora de 0..1: {}", s.pos));
            }
            if let ColorRef::Direct { rgba } = &s.color {
                if !rgba.is_valid() {
                    return Err("cor de parada fora de 0..1".into());
                }
            }
        }
        if !self.scale.is_finite() || self.scale <= 0.0 {
            return Err("escala do degradê deve ser > 0".into());
        }
        Ok(())
    }

    /// Resolve as cores (paleta → RGBA) e prepara a avaliação.
    pub fn resolve(&self, mut color: impl FnMut(&ColorRef) -> [f32; 4]) -> ResolvedGradient {
        let g = self.clone().sanitized();
        let stops: Vec<(f32, [f32; 4])> = g
            .stops
            .iter()
            .map(|s| {
                let c = color(&s.color);
                (s.pos, [c[0], c[1], c[2], (c[3] * s.opacity).clamp(0.0, 1.0)])
            })
            .collect();
        ResolvedGradient::new(stops, g.shape, g.extend, g.space, g.angle, g.center, g.scale, g.absolute)
    }

    /// Cópia com as cores de paleta trocadas pelas diretas (para quem não enxerga o projeto, como o pincel).
    pub fn with_direct_colors(&self, mut color: impl FnMut(&ColorRef) -> [f32; 4]) -> Gradient {
        let mut g = self.clone();
        for s in &mut g.stops {
            s.color = ColorRef::Direct { rgba: Rgba(color(&s.color)) };
        }
        g
    }

    /// Paradas invertidas (pos → 1 - pos).
    pub fn reversed(&self) -> Gradient {
        let mut g = self.clone();
        for s in &mut g.stops {
            s.pos = 1.0 - s.pos;
        }
        g.stops.reverse();
        g
    }
}

/// Degradê com nome guardado numa paleta (`PaletteData.gradients`) e na biblioteca junto com ela.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NamedGradient {
    #[serde(default = "no_id")]
    pub id: Id,
    pub name: String,
    pub gradient: Gradient,
}

/// Tinta: o que preenche uma forma.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Paint {
    Solid {
        color: ColorRef,
    },
    Gradient {
        gradient: Gradient,
    },
    /// Imagem (textura do projeto/biblioteca) repetida em ladrilho.
    Pattern {
        /// Blob PNG da imagem (ativo de textura do projeto).
        image: BlobHash,
        /// 1 = tamanho natural.
        #[serde(default = "one")]
        scale: f32,
        /// Deslocamento do ladrilho em px do canvas.
        #[serde(default)]
        offset: [f32; 2],
        /// Graus.
        #[serde(default)]
        angle: f32,
    },
}

impl Default for Paint {
    fn default() -> Self {
        Paint::Solid { color: ColorRef::default() }
    }
}

impl Paint {
    pub fn solid(rgba: [f32; 4]) -> Paint {
        Paint::Solid { color: ColorRef::Direct { rgba: Rgba(rgba) } }
    }

    pub fn gradient(&self) -> Option<&Gradient> {
        match self {
            Paint::Gradient { gradient } => Some(gradient),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------------------------------------ avaliação

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.max(0.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// sRGB (0..1) → OKLab.
pub fn srgb_to_oklab(c: [f32; 3]) -> [f32; 3] {
    let (r, g, b) = (srgb_to_linear(c[0]), srgb_to_linear(c[1]), srgb_to_linear(c[2]));
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab → sRGB (0..1, preso).
pub fn oklab_to_srgb(c: [f32; 3]) -> [f32; 3] {
    let l = c[0] + 0.396_337_78 * c[1] + 0.215_803_76 * c[2];
    let m = c[0] - 0.105_561_346 * c[1] - 0.063_854_17 * c[2];
    let s = c[0] - 0.089_484_18 * c[1] - 1.291_485_5 * c[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    [linear_to_srgb(r).clamp(0.0, 1.0), linear_to_srgb(g).clamp(0.0, 1.0), linear_to_srgb(b).clamp(0.0, 1.0)]
}

const LUT_N: usize = 1024;

/// Degradê pronto para avaliar: paradas resolvidas (sRGB não pré-multiplicado) e a geometria.
#[derive(Debug, Clone)]
pub struct ResolvedGradient {
    pub stops: Vec<(f32, [f32; 4])>,
    pub shape: GradientShape,
    pub extend: GradientExtend,
    pub space: GradientSpace,
    pub angle: f32,
    pub center: [f32; 2],
    pub scale: f32,
    pub absolute: Option<[[f32; 2]; 2]>,
    /// Tabela (só `linear`/`oklab`): cor sRGB não pré-multiplicada em `LUT_N + 1` pontos de 0..1.
    lut: Option<Vec<[f32; 4]>>,
}

impl ResolvedGradient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(mut stops: Vec<(f32, [f32; 4])>, shape: GradientShape, extend: GradientExtend, space: GradientSpace, angle: f32, center: [f32; 2], scale: f32, absolute: Option<[[f32; 2]; 2]>) -> ResolvedGradient {
        stops.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut g = ResolvedGradient { stops, shape, extend, space, angle, center, scale, absolute, lut: None };
        if space != GradientSpace::Srgb && !g.stops.is_empty() {
            g.lut = Some((0..=LUT_N).map(|i| g.interp(i as f32 / LUT_N as f32)).collect());
        }
        g
    }

    /// Linear entre dois pontos do canvas, paradas sRGB, preso nas pontas: o degradê da ferramenta Degradê.
    pub fn simple(stops: &[(f32, [f32; 4])], shape: GradientShape, p0: [f32; 2], p1: [f32; 2]) -> ResolvedGradient {
        ResolvedGradient::new(stops.to_vec(), shape, GradientExtend::Pad, GradientSpace::Srgb, 0.0, [0.5, 0.5], 1.0, Some([p0, p1]))
    }

    /// Aplica a repetição a `t` (fora de 0..1).
    pub fn wrap(&self, t: f32) -> f32 {
        if !t.is_finite() {
            return 0.0;
        }
        match self.extend {
            GradientExtend::Pad => t.clamp(0.0, 1.0),
            GradientExtend::Repeat => {
                let f = t.rem_euclid(1.0);
                // t = 1 exato (fim da faixa) fica no fim, não volta ao começo
                if f == 0.0 && t > 0.0 { 1.0 } else { f }
            }
            GradientExtend::Reflect => {
                let f = t.rem_euclid(2.0);
                if f > 1.0 { 2.0 - f } else { f }
            }
        }
    }

    /// Cor (sRGB não pré-multiplicada) na posição `t` (já com a repetição aplicada por `color_at`).
    fn interp(&self, t: f32) -> [f32; 4] {
        let stops = &self.stops;
        if stops.is_empty() {
            return [0.0; 4];
        }
        if t <= stops[0].0 {
            return stops[0].1;
        }
        for w in stops.windows(2) {
            if t <= w[1].0 {
                let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                return mix(w[0].1, w[1].1, f, self.space);
            }
        }
        stops[stops.len() - 1].1
    }

    /// Cor (sRGB não pré-multiplicada) em `t` do eixo do degradê (antes da repetição).
    pub fn color_at(&self, t: f32) -> [f32; 4] {
        let t = self.wrap(t);
        match &self.lut {
            None => self.interp(t),
            Some(lut) => {
                let x = t * LUT_N as f32;
                let i = (x.floor() as usize).min(LUT_N - 1);
                let f = x - i as f32;
                let (a, b) = (lut[i], lut[i + 1]);
                [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * f)
            }
        }
    }

    /// O mapeador ponto → `t` para uma caixa [x0, y0, x1, y1] (ignorada com `absolute`).
    pub fn mapper(&self, bbox: [f32; 4]) -> GradientMapper {
        let (s, c) = self.angle.to_radians().sin_cos();
        match self.absolute {
            Some([p0, p1]) => {
                let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
                let len2 = (dx * dx + dy * dy).max(1e-6);
                let len = len2.sqrt();
                GradientMapper { shape: self.shape, origin: p0, dir: [dx / len, dy / len], len: if self.shape == GradientShape::Angular { 1.0 } else { len }, offset: 0.0, base_angle: dy.atan2(dx), lin: Some([dx, dy, len2]) }
            }
            None => {
                let (w, h) = ((bbox[2] - bbox[0]).abs().max(1e-3), (bbox[3] - bbox[1]).abs().max(1e-3));
                let origin = [bbox[0] + w * self.center[0], bbox[1] + h * self.center[1]];
                let k = self.scale.max(1e-3);
                match self.shape {
                    GradientShape::Linear => {
                        // meia extensão da caixa projetada na direção; t vai de 0 numa borda a 1 na outra
                        let half = ((w * c.abs() + h * s.abs()) / 2.0 * k).max(1e-3);
                        GradientMapper { shape: self.shape, origin, dir: [c, s], len: 2.0 * half, offset: 0.5, base_angle: 0.0, lin: None }
                    }
                    GradientShape::Radial => {
                        let r = ((w * w + h * h).sqrt() / 2.0 * k).max(1e-3);
                        GradientMapper { shape: self.shape, origin, dir: [c, s], len: r, offset: 0.0, base_angle: 0.0, lin: None }
                    }
                    GradientShape::Angular => GradientMapper { shape: self.shape, origin, dir: [c, s], len: k, offset: 0.0, base_angle: self.angle.to_radians(), lin: None },
                    GradientShape::Diamond => {
                        let r = ((w + h) / 2.0 * k).max(1e-3);
                        GradientMapper { shape: self.shape, origin, dir: [c, s], len: r, offset: 0.0, base_angle: 0.0, lin: None }
                    }
                }
            }
        }
    }

    /// Cor (sRGB não pré-multiplicada) no ponto `p` com o mapeador `m`.
    #[inline]
    pub fn sample(&self, m: &GradientMapper, p: [f32; 2]) -> [f32; 4] {
        self.color_at(m.t(p))
    }

    /// Cor pré-multiplicada.
    #[inline]
    pub fn sample_premul(&self, m: &GradientMapper, p: [f32; 2]) -> [f32; 4] {
        let c = self.sample(m, p);
        [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]]
    }

    /// Alguma parada tem alfa < 1.
    pub fn has_alpha(&self) -> bool {
        self.stops.iter().any(|s| s.1[3] < 1.0)
    }
}

/// Ponto do canvas → posição no eixo do degradê.
#[derive(Debug, Clone, Copy)]
pub struct GradientMapper {
    shape: GradientShape,
    origin: [f32; 2],
    dir: [f32; 2],
    /// Comprimento do eixo (divide: a mesma conta do `gradient_fill` antigo).
    len: f32,
    offset: f32,
    base_angle: f32,
    /// Linear absoluto: projeção exata (dx, dy) / |d|² do `gradient_fill` antigo.
    lin: Option<[f32; 3]>,
}

impl GradientMapper {
    #[inline]
    pub fn t(&self, p: [f32; 2]) -> f32 {
        let (dx, dy) = (p[0] - self.origin[0], p[1] - self.origin[1]);
        match self.shape {
            GradientShape::Linear => match self.lin {
                Some(l) => (dx * l[0] + dy * l[1]) / l[2],
                None => (dx * self.dir[0] + dy * self.dir[1]) / self.len + self.offset,
            },
            GradientShape::Radial => (dx * dx + dy * dy).sqrt() / self.len,
            GradientShape::Angular => {
                let a = dy.atan2(dx) - self.base_angle;
                (a / std::f32::consts::TAU).rem_euclid(1.0) / self.len
            }
            GradientShape::Diamond => {
                // referencial girado pela direção
                let u = dx * self.dir[0] + dy * self.dir[1];
                let v = -dx * self.dir[1] + dy * self.dir[0];
                (u.abs() + v.abs()) / self.len
            }
        }
    }
}

fn mix(a: [f32; 4], b: [f32; 4], f: f32, space: GradientSpace) -> [f32; 4] {
    match space {
        // o comportamento antigo: componentes sRGB não pré-multiplicados
        GradientSpace::Srgb => [0, 1, 2, 3].map(|k| a[k] + (b[k] - a[k]) * f),
        GradientSpace::Linear => {
            let la = [srgb_to_linear(a[0]) * a[3], srgb_to_linear(a[1]) * a[3], srgb_to_linear(a[2]) * a[3]];
            let lb = [srgb_to_linear(b[0]) * b[3], srgb_to_linear(b[1]) * b[3], srgb_to_linear(b[2]) * b[3]];
            let al = a[3] + (b[3] - a[3]) * f;
            if al <= 1e-6 {
                return [0.0; 4];
            }
            let c = [0, 1, 2].map(|k| linear_to_srgb((la[k] + (lb[k] - la[k]) * f) / al).clamp(0.0, 1.0));
            [c[0], c[1], c[2], al]
        }
        GradientSpace::Oklab => {
            let (oa, ob) = (srgb_to_oklab([a[0], a[1], a[2]]), srgb_to_oklab([b[0], b[1], b[2]]));
            let al = a[3] + (b[3] - a[3]) * f;
            if al <= 1e-6 {
                return [0.0; 4];
            }
            let lab = [0, 1, 2].map(|k| (oa[k] * a[3] + (ob[k] * b[3] - oa[k] * a[3]) * f) / al);
            let c = oklab_to_srgb(lab);
            [c[0], c[1], c[2], al]
        }
    }
}

/// Degradês prontos (desenhados aqui, procedurais).
pub fn builtin_gradients() -> Vec<(&'static str, Gradient)> {
    let h = |s: &str| Rgba::from_hex(s).map(|c| c.0).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let lin = |cols: &[&str], angle: f32, space: GradientSpace| Gradient { space, ..Gradient::from_colors(&cols.iter().map(|c| h(c)).collect::<Vec<_>>(), angle) };
    vec![
        ("Preto → branco", Gradient::default()),
        ("Pôr do sol", lin(&["#ffd36b", "#ff7a3d", "#c2335f", "#3b1f5c"], 90.0, GradientSpace::Oklab)),
        ("Céu de dia", lin(&["#3d7fd9", "#8fc4f2", "#e8f4ff"], 90.0, GradientSpace::Oklab)),
        ("Ouro", Gradient { extend: GradientExtend::Pad, ..lin(&["#fff4c2", "#f2c14e", "#b07a19", "#f8dc7a", "#8a5a12"], 90.0, GradientSpace::Srgb) }),
        ("Prata", lin(&["#ffffff", "#c9ced6", "#7d8590", "#e3e7ec", "#59606b"], 90.0, GradientSpace::Srgb)),
        ("Arco-íris", Gradient { stops: ["#ff3b3b", "#ffa02e", "#ffe53b", "#3bd16f", "#3b9dff", "#8a4dff"].iter().enumerate().map(|(i, c)| GradientStop::direct(i as f32 / 5.0, h(c))).collect(), space: GradientSpace::Oklab, ..Default::default() }),
        ("Mahō shōjo", lin(&["#ff8ad8", "#c59bff", "#7fe3ff"], 90.0, GradientSpace::Oklab)),
        ("Neon", lin(&["#00f0ff", "#ff00d4"], 0.0, GradientSpace::Oklab)),
        ("Fogo", lin(&["#fff3a0", "#ffb000", "#ff4a00", "#8a0000"], 270.0, GradientSpace::Linear)),
        ("Cor → transparente", Gradient { stops: vec![GradientStop::direct(0.0, [1.0, 1.0, 1.0, 1.0]), GradientStop::direct(1.0, [1.0, 1.0, 1.0, 0.0])], space: GradientSpace::Linear, ..Default::default() }),
        ("Brilho radial", Gradient { shape: GradientShape::Radial, stops: vec![GradientStop::direct(0.0, [1.0, 1.0, 0.9, 1.0]), GradientStop::direct(0.4, [1.0, 0.85, 0.4, 0.8]), GradientStop::direct(1.0, [1.0, 0.6, 0.2, 0.0])], space: GradientSpace::Linear, ..Default::default() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bw() -> Gradient {
        Gradient::default()
    }

    fn rg(g: &Gradient) -> ResolvedGradient {
        g.resolve(|c| match c {
            ColorRef::Direct { rgba } => rgba.0,
            ColorRef::Palette { .. } => [1.0, 0.0, 1.0, 1.0],
        })
    }

    fn close(a: [f32; 4], b: [f32; 4], tol: f32) -> bool {
        (0..4).all(|k| (a[k] - b[k]).abs() <= tol)
    }

    #[test]
    fn stops_interpolate_and_pad_outside() {
        let g = rg(&Gradient { stops: vec![GradientStop::direct(0.25, [1.0, 0.0, 0.0, 1.0]), GradientStop::direct(0.75, [0.0, 0.0, 1.0, 1.0])], ..bw() });
        assert!(close(g.color_at(0.0), [1.0, 0.0, 0.0, 1.0], 1e-6), "antes da 1ª parada: a cor dela");
        assert!(close(g.color_at(0.5), [0.5, 0.0, 0.5, 1.0], 1e-6));
        assert!(close(g.color_at(1.5), [0.0, 0.0, 1.0, 1.0], 1e-6), "pad");
        // três paradas: a do meio manda
        let g3 = rg(&Gradient::from_colors(&[[0.0; 4], [1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0]], 0.0));
        assert!(close(g3.color_at(0.5), [1.0, 1.0, 1.0, 1.0], 1e-6));
        // paradas fora de ordem são ordenadas
        let rev = rg(&Gradient { stops: vec![GradientStop::direct(1.0, [1.0; 4]), GradientStop::direct(0.0, [0.0, 0.0, 0.0, 1.0])], ..bw() });
        assert!(rev.color_at(0.1)[0] < 0.2);
    }

    #[test]
    fn repeat_and_reflect_wrap_the_axis() {
        let rep = rg(&Gradient { extend: GradientExtend::Repeat, ..bw() });
        assert!((rep.color_at(1.25)[0] - 0.25).abs() < 1e-5);
        assert!((rep.color_at(-0.25)[0] - 0.75).abs() < 1e-5);
        assert!((rep.color_at(1.0)[0] - 1.0).abs() < 1e-6, "o fim exato fica no fim");
        let refl = rg(&Gradient { extend: GradientExtend::Reflect, ..bw() });
        assert!((refl.color_at(1.25)[0] - 0.75).abs() < 1e-5);
        assert!((refl.color_at(2.25)[0] - 0.25).abs() < 1e-5);
        assert!((refl.color_at(-0.25)[0] - 0.25).abs() < 1e-5);
    }

    #[test]
    fn spaces_differ_in_the_middle_and_agree_at_the_ends() {
        let mk = |space| rg(&Gradient { space, ..Gradient::from_colors(&[[1.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]], 0.0) });
        let (s, l, o) = (mk(GradientSpace::Srgb), mk(GradientSpace::Linear), mk(GradientSpace::Oklab));
        for g in [&s, &l, &o] {
            assert!(close(g.color_at(0.0), [1.0, 0.0, 0.0, 1.0], 2e-3) && close(g.color_at(1.0), [0.0, 1.0, 0.0, 1.0], 2e-3));
        }
        let lum = |c: [f32; 4]| 0.2126 * srgb_to_linear(c[0]) + 0.7152 * srgb_to_linear(c[1]) + 0.0722 * srgb_to_linear(c[2]);
        // sRGB puro escurece no meio (0,5 + 0,5 em sRGB = ~0,21 linear cada); linear e OKLab não
        assert!(lum(l.color_at(0.5)) > lum(s.color_at(0.5)) + 0.1, "linear mais claro no meio: {:?} vs {:?}", l.color_at(0.5), s.color_at(0.5));
        assert!(lum(o.color_at(0.5)) > lum(s.color_at(0.5)) + 0.05);
        assert!(!close(l.color_at(0.5), o.color_at(0.5), 0.01), "linear e OKLab são caminhos diferentes");
        // OKLab ida e volta
        for c in [[0.2f32, 0.5, 0.9], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.9, 0.1, 0.3]] {
            let back = oklab_to_srgb(srgb_to_oklab(c));
            assert!((0..3).all(|k| (back[k] - c[k]).abs() < 2e-3), "{c:?} → {back:?}");
        }
    }

    #[test]
    fn premultiplied_spaces_avoid_dark_fringes_with_transparent_stops() {
        // vermelho → transparente PRETO: em sRGB a borda escurece, em linear pré-multiplicado fica vermelha
        let mk = |space| rg(&Gradient { space, stops: vec![GradientStop::direct(0.0, [1.0, 0.0, 0.0, 1.0]), GradientStop::direct(1.0, [0.0, 0.0, 0.0, 0.0])], ..bw() });
        assert!(mk(GradientSpace::Srgb).color_at(0.5)[0] < 0.6);
        assert!(mk(GradientSpace::Linear).color_at(0.5)[0] > 0.99);
        assert!(mk(GradientSpace::Oklab).color_at(0.5)[0] > 0.95);
    }

    #[test]
    fn shapes_map_points_on_the_box() {
        let bbox = [0.0, 0.0, 100.0, 50.0];
        let lin = rg(&bw());
        let m = lin.mapper(bbox);
        assert!(m.t([0.0, 25.0]).abs() < 1e-4 && (m.t([100.0, 25.0]) - 1.0).abs() < 1e-4 && (m.t([50.0, 0.0]) - 0.5).abs() < 1e-4);
        let down = rg(&Gradient { angle: 90.0, ..bw() });
        let md = down.mapper(bbox);
        assert!(md.t([10.0, 0.0]).abs() < 1e-3 && (md.t([10.0, 50.0]) - 1.0).abs() < 1e-3, "90° = de cima para baixo");
        let rad = rg(&Gradient { shape: GradientShape::Radial, ..bw() });
        let mr = rad.mapper(bbox);
        assert!(mr.t([50.0, 25.0]).abs() < 1e-4 && (mr.t([100.0, 50.0]) - 1.0).abs() < 1e-3, "centro 0, canto 1");
        let ang = rg(&Gradient { shape: GradientShape::Angular, ..bw() });
        let ma = ang.mapper(bbox);
        assert!((ma.t([50.0, 50.0]) - 0.25).abs() < 1e-3, "um quarto de volta (horário) para baixo");
        assert!((ma.t([0.0, 25.0]) - 0.5).abs() < 1e-3);
        let dia = rg(&Gradient { shape: GradientShape::Diamond, ..bw() });
        let mm = dia.mapper(bbox);
        assert!((mm.t([100.0, 50.0]) - 1.0).abs() < 1e-3 && (mm.t([75.0, 25.0]) - 1.0 / 3.0).abs() < 1e-3);
        // absoluto = o gradient_fill de sempre
        let abs = ResolvedGradient::simple(&[(0.0, [0.0, 0.0, 0.0, 1.0]), (1.0, [1.0; 4])], GradientShape::Linear, [10.0, 0.0], [110.0, 0.0]);
        let ma = abs.mapper([0.0; 4]);
        assert!((ma.t([60.0, 33.0]) - 0.5).abs() < 1e-6);
        // escala 0,5 + repetir: duas faixas na caixa
        let half = rg(&Gradient { scale: 0.5, extend: GradientExtend::Repeat, ..bw() });
        let mh = half.mapper(bbox);
        assert!((half.sample(&mh, [50.0, 25.0])[0] - 0.5).abs() < 1e-3);
        assert!(half.sample(&mh, [75.0 - 0.01, 25.0])[0] > 0.98 && half.sample(&mh, [75.0 + 0.5, 25.0])[0] < 0.05, "repetiu no quarto da caixa");
    }

    #[test]
    fn serde_round_trip_and_partial_json() {
        let g = Gradient { shape: GradientShape::Diamond, angle: 33.0, extend: GradientExtend::Reflect, space: GradientSpace::Oklab, absolute: Some([[1.0, 2.0], [3.0, 4.0]]), ..Gradient::from_colors(&[[0.1, 0.2, 0.3, 0.4], [1.0; 4]], 0.0) };
        let v = serde_json::to_value(&g).unwrap();
        assert_eq!(serde_json::from_value::<Gradient>(v).unwrap(), g);
        let partial: Gradient = serde_json::from_value(serde_json::json!({"shape": "radial"})).unwrap();
        assert_eq!((partial.shape, partial.stops.len(), partial.space, partial.scale), (GradientShape::Radial, 2, GradientSpace::Srgb, 1.0));
        let p = Paint::Gradient { gradient: g.clone() };
        let pv = serde_json::to_value(&p).unwrap();
        assert_eq!(pv["type"], "gradient");
        assert_eq!(serde_json::from_value::<Paint>(pv).unwrap(), p);
        let s: Paint = serde_json::from_value(serde_json::json!({"type": "solid", "color": {"type": "direct", "rgba": [1, 0, 0, 1]}})).unwrap();
        assert_eq!(s, Paint::solid([1.0, 0.0, 0.0, 1.0]));
        // `absolute` ausente viaja como null (o merge patch apaga o valor antigo)
        assert!(serde_json::to_value(Gradient::default()).unwrap().get("absolute").is_some_and(|v| v.is_null()));
    }

    #[test]
    fn palette_stops_resolve_through_the_callback() {
        let g = Gradient { stops: vec![GradientStop { pos: 0.0, color: ColorRef::Palette { palette: Id::from("p"), color: Id::from("c") }, opacity: 0.5 }, GradientStop::direct(1.0, [0.0, 0.0, 1.0, 1.0])], ..bw() };
        let r = g.resolve(|c| match c {
            ColorRef::Palette { .. } => [0.0, 1.0, 0.0, 1.0],
            ColorRef::Direct { rgba } => rgba.0,
        });
        assert!(close(r.color_at(0.0), [0.0, 1.0, 0.0, 0.5], 1e-6), "a opacidade da parada multiplica o alfa");
        let d = g.with_direct_colors(|_| [0.3, 0.3, 0.3, 1.0]);
        assert!(d.stops.iter().all(|s| matches!(s.color, ColorRef::Direct { .. })));
        assert!(g.validate().is_ok() && Gradient { stops: vec![], ..bw() }.validate().is_err());
    }
}
