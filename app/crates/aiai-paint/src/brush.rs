//! Motor de pincel raster por carimbos ("dabs"), inspirado no painel Brush Settings do Photoshop e no modelo de dab do
//! libmypaint (leitura de comportamento; implementação própria):
//!
//! - ponta procedural (diâmetro, dureza, redondeza, ângulo) ou por máscara de imagem;
//! - espaçamento em fração do diâmetro; dinâmicas de pressão → tamanho, opacidade, fluxo;
//! - dispersão, jitter de tamanho/ângulo/cor (RNG com semente → determinístico), ângulo seguindo a direção;
//! - textura de pincel em espaço de tela (multiplicar/subtrair, escala, profundidade) e pincel duplo;
//! - mistura com a cor da camada (borrar/misturar) e bordas molhadas (aquarela);
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
fn shape_alpha(shape: TipShape, u: f32, v: f32, aa: f32) -> f32 {
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
            let c = |cx: f32, cy: f32, r: f32| ((u - cx).powi(2) + (v - cy).powi(2)).sqrt() - r;
            let d = c(0.0, -0.1, 0.55).min(c(-0.45, 0.2, 0.42)).min(c(0.45, 0.2, 0.45)).min(c(0.0, 0.35, 0.5));
            edge(d)
        }
        TipShape::Star => {
            let r = (u * u + v * v).sqrt();
            let a = v.atan2(u);
            let k = (a * 5.0 / 2.0).cos().abs().powf(8.0);
            let rad = 0.25 + 0.75 * k;
            edge(r - rad)
        }
        TipShape::Square => edge(u.abs().max(v.abs()) - 0.9),
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DualBrush {
    pub size: f32,
    pub spacing: f32,
    pub scatter: f32,
    pub hardness: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushPreset {
    pub name: String,
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
        }
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
        if self.count == 0 || self.count > 64 {
            return Err("count deve estar entre 1 e 64".into());
        }
        Ok(())
    }

    /// Presets iniciais do programa (S06).
    pub fn defaults() -> Vec<BrushPreset> {
        let mut v = Vec::new();
        let mut pencil = BrushPreset::round("Lápis de rascunho", 3.0, 0.9);
        pencil.opacity_pressure_min = 0.3;
        pencil.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 1.5, depth: 0.6, mode: TextureMode::Multiply, seed: 3, invert: false });
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
        wc.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 6.0, depth: 0.35, mode: TextureMode::Multiply, seed: 7, invert: false });
        v.push(wc);
        let mut gouache = BrushPreset::round("Guache seco", 36.0, 0.75);
        gouache.tip = BrushTip::Round { hardness: 0.75, roundness: 0.55, angle: 35.0 };
        gouache.follow_direction = true;
        gouache.texture = Some(BrushTexture { kind: TextureKind::Canvas, scale: 3.0, depth: 0.55, mode: TextureMode::Subtract, seed: 11, invert: false });
        gouache.opacity_pressure_min = 0.5;
        v.push(gouache);
        let mut charcoal = BrushPreset::round("Carvão", 14.0, 0.6);
        charcoal.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 2.5, depth: 0.85, mode: TextureMode::Subtract, seed: 5, invert: false });
        charcoal.scatter = 0.1;
        charcoal.opacity_pressure_min = 0.2;
        v.push(charcoal);
        let mut paper = BrushPreset::round("Textura de papel", 80.0, 0.3);
        paper.flow = 0.4;
        paper.texture = Some(BrushTexture { kind: TextureKind::Paper, scale: 10.0, depth: 0.9, mode: TextureMode::Multiply, seed: 13, invert: false });
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
        v.push(foliage);
        let mut grass = BrushPreset::round("Grama", 30.0, 1.0);
        grass.tip = BrushTip::Shape { shape: TipShape::GrassBlade, aspect: 1.0 };
        grass.spacing = 0.12;
        grass.scatter = 0.3;
        grass.count = 2;
        grass.size_jitter = 0.35;
        grass.angle_jitter = 12.0;
        grass.value_jitter = 0.15;
        v.push(grass);
        let mut rose = BrushPreset::round("Rosas", 18.0, 1.0);
        rose.tip = BrushTip::Shape { shape: TipShape::Rose, aspect: 1.0 };
        rose.spacing = 0.9;
        rose.scatter = 0.8;
        rose.count = 2;
        rose.size_jitter = 0.35;
        rose.angle_jitter = 180.0;
        rose.value_jitter = 0.1;
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
        cloud.flow = 0.6;
        v.push(cloud);
        let mut sparkle = BrushPreset::round("Brilhos", 18.0, 1.0);
        sparkle.tip = BrushTip::Shape { shape: TipShape::Star, aspect: 1.0 };
        sparkle.spacing = 1.5;
        sparkle.scatter = 2.0;
        sparkle.size_jitter = 0.6;
        sparkle.angle_jitter = 20.0;
        v.push(sparkle);
        let mut eraser = BrushPreset::round("Borracha", 30.0, 0.9);
        eraser.mode = BrushMode::Erase;
        v.push(eraser);
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

fn tip_alpha(tip: &BrushTip, dx: f32, dy: f32, radius: f32, extra_angle: f32, aliased: bool) -> f32 {
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
        BrushTip::Shape { shape, aspect } => {
            let a = extra_angle.to_radians();
            let (sn, cs) = a.sin_cos();
            let lx = dx * cs + dy * sn;
            let ly = -dx * sn + dy * cs;
            let r = radius.max(0.25);
            let (u, v) = (lx / (r * aspect.clamp(0.1, 4.0)), ly / r);
            let val = shape_alpha(*shape, u, v, 1.0 / r);
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
    if path.is_empty() {
        return None;
    }
    let (w, h) = (layer.width as i32, layer.height as i32);
    let max_r = preset.size * 0.5 * (1.0 + preset.size_jitter) + preset.scatter * preset.size + 2.0;
    let (mut x0, mut y0, mut x1, mut y1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
    for p in path {
        x0 = x0.min((p.x - max_r).floor() as i32);
        y0 = y0.min((p.y - max_r).floor() as i32);
        x1 = x1.max((p.x + max_r).ceil() as i32 + 1);
        y1 = y1.max((p.y + max_r).ceil() as i32 + 1);
    }
    let (x0, y0, x1, y1) = (x0.max(0), y0.max(0), x1.min(w), y1.min(h));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (bw, bh) = ((x1 - x0) as usize, (y1 - y0) as usize);
    // buffer do traço, pré-multiplicado
    let mut buf = vec![[0.0f32; 4]; bw * bh];
    let mut dual_buf = preset.dual.as_ref().map(|_| vec![0.0f32; bw * bh]);
    let mut rng = Rng::new(seed);
    let mut carry: Option<[f32; 3]> = None;

    // dabs ao longo do caminho com espaçamento por distância
    let mut dabs: Vec<(f32, f32, f32, f32)> = Vec::new(); // x, y, pressão, direção (graus)
    let mut acc = 0.0f32;
    let first = path[0];
    dabs.push((first.x, first.y, first.pressure, 0.0));
    for seg in path.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-6 {
            continue;
        }
        let dir = dy.atan2(dx).to_degrees();
        let mut t = 0.0f32;
        loop {
            let pr = a.pressure + (b.pressure - a.pressure) * (t / len);
            let diam = preset.size * (preset.size_pressure_min + (1.0 - preset.size_pressure_min) * pr.clamp(0.0, 1.0));
            let step = (diam * preset.spacing).max(0.5);
            let need = step - acc;
            if t + need > len {
                acc += len - t;
                break;
            }
            t += need;
            acc = 0.0;
            let f = t / len;
            dabs.push((a.x + dx * f, a.y + dy * f, a.pressure + (b.pressure - a.pressure) * f, dir));
        }
    }

    for &(cx, cy, pr, dir) in &dabs {
        let pr = pr.clamp(0.0, 1.0);
        for _ in 0..preset.count {
            let mut size = preset.size * (preset.size_pressure_min + (1.0 - preset.size_pressure_min) * pr);
            if preset.size_jitter > 0.0 {
                size *= 1.0 + preset.size_jitter * rng.signed();
            }
            let r = (size * 0.5).max(0.3);
            let (mut px, mut py) = (cx, cy);
            if preset.scatter > 0.0 {
                let perp = (dir + 90.0).to_radians();
                let off = preset.scatter * preset.size * rng.signed();
                px += perp.cos() * off;
                py += perp.sin() * off;
                if preset.count > 1 {
                    px += preset.scatter * preset.size * 0.5 * rng.signed();
                }
            }
            let mut angle = if preset.follow_direction { dir } else { 0.0 };
            if preset.angle_jitter > 0.0 {
                angle += preset.angle_jitter * rng.signed();
            }
            let dab_opacity = preset.opacity_pressure_min + (1.0 - preset.opacity_pressure_min) * pr;
            let flow = preset.flow * (preset.flow_pressure_min + (1.0 - preset.flow_pressure_min) * pr);
            // cor do dab
            let mut rgb = [color[0], color[1], color[2]];
            if preset.hue_jitter > 0.0 || preset.saturation_jitter > 0.0 || preset.value_jitter > 0.0 {
                let mut hsv = rgb_to_hsv(rgb);
                hsv[0] += preset.hue_jitter * 0.5 * rng.signed();
                hsv[1] = (hsv[1] + preset.saturation_jitter * rng.signed()).clamp(0.0, 1.0);
                hsv[2] = (hsv[2] + preset.value_jitter * rng.signed()).clamp(0.0, 1.0);
                rgb = hsv_to_rgb(hsv);
            }
            if preset.mix > 0.0 {
                // média da camada sob o dab
                let (sx, sy) = (px.round() as i32, py.round() as i32);
                let rr = (r * 0.5).max(1.0) as i32;
                let mut sum = [0.0f32; 4];
                let mut n = 0.0;
                for yy in (sy - rr).max(0)..(sy + rr + 1).min(h) {
                    for xx in (sx - rr).max(0)..(sx + rr + 1).min(w) {
                        let p = layer.get(xx as u32, yy as u32);
                        for c in 0..4 {
                            sum[c] += p[c];
                        }
                        n += 1.0;
                    }
                }
                if n > 0.0 && sum[3] > 0.0 {
                    let under = [sum[0] / sum[3], sum[1] / sum[3], sum[2] / sum[3]];
                    let base = match carry {
                        Some(c) => [c[0] + (under[0] - c[0]) * (1.0 - preset.smudge_carry), c[1] + (under[1] - c[1]) * (1.0 - preset.smudge_carry), c[2] + (under[2] - c[2]) * (1.0 - preset.smudge_carry)],
                        None => under,
                    };
                    for c in 0..3 {
                        rgb[c] += (base[c] - rgb[c]) * preset.mix;
                    }
                    carry = Some(rgb);
                }
            }
            // carimbo
            let (bx0, by0) = (((px - r - 1.0).floor() as i32).max(x0), ((py - r - 1.0).floor() as i32).max(y0));
            let (bx1, by1) = (((px + r + 1.0).ceil() as i32).min(x1), ((py + r + 1.0).ceil() as i32).min(y1));
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (dx, dy) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                    let mut a = tip_alpha(&preset.tip, dx, dy, r, angle, preset.aliased);
                    if a <= 0.0 {
                        continue;
                    }
                    if let Some(t) = &preset.texture {
                        let tv = texture_value(t, x as f32, y as f32);
                        a = match t.mode {
                            TextureMode::Multiply => a * (1.0 - t.depth + t.depth * tv),
                            TextureMode::Subtract => (a - (1.0 - tv) * t.depth).max(0.0),
                        };
                    }
                    let da = (a * flow * dab_opacity * color[3]).clamp(0.0, 1.0);
                    if da <= 0.0 {
                        continue;
                    }
                    let i = (y - y0) as usize * bw + (x - x0) as usize;
                    let d = &mut buf[i];
                    let k = 1.0 - da;
                    d[0] = rgb[0] * da + d[0] * k;
                    d[1] = rgb[1] * da + d[1] * k;
                    d[2] = rgb[2] * da + d[2] * k;
                    d[3] = da + d[3] * k;
                }
            }
        }
    }

    // pincel duplo: segunda ponta ao longo do caminho; o traço só aparece onde as duas se sobrepõem
    if let (Some(dual), Some(db)) = (&preset.dual, dual_buf.as_mut()) {
        let r = (dual.size * 0.5).max(0.3);
        let step = (dual.size * dual.spacing).max(0.5);
        let mut drng = Rng::new(seed ^ 0xD0A1);
        for seg in path.windows(2) {
            let (a, b) = (seg[0], seg[1]);
            let len = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            let n = (len / step).ceil().max(1.0) as usize;
            for k in 0..n {
                let f = k as f32 / n as f32;
                let px = a.x + (b.x - a.x) * f + dual.scatter * dual.size * drng.signed();
                let py = a.y + (b.y - a.y) * f + dual.scatter * dual.size * drng.signed();
                let tip = BrushTip::Round { hardness: dual.hardness, roundness: 1.0, angle: 0.0 };
                for y in ((py - r - 1.0) as i32).max(y0)..((py + r + 1.0) as i32 + 1).min(y1) {
                    for x in ((px - r - 1.0) as i32).max(x0)..((px + r + 1.0) as i32 + 1).min(x1) {
                        let a2 = tip_alpha(&tip, x as f32 + 0.5 - px, y as f32 + 0.5 - py, r, 0.0, false);
                        let i = (y - y0) as usize * bw + (x - x0) as usize;
                        db[i] = db[i].max(a2);
                    }
                }
            }
        }
        for (p, m) in buf.iter_mut().zip(db.iter()) {
            for c in p.iter_mut() {
                *c *= m;
            }
        }
    }

    // bordas molhadas: centro mais claro, borda mais carregada
    if preset.wet_edges > 0.0 {
        let rad = (preset.size * 0.12).clamp(1.0, 12.0) as i32;
        let alpha: Vec<f32> = buf.iter().map(|p| p[3]).collect();
        let blurred = box_blur(&alpha, bw, bh, rad);
        for (i, p) in buf.iter_mut().enumerate() {
            if p[3] <= 0.0 {
                continue;
            }
            let edge = ((alpha[i] - blurred[i]).max(0.0) * 3.0).min(1.0);
            let factor = (1.0 - preset.wet_edges * 0.5) + preset.wet_edges * 0.8 * edge;
            let na = (p[3] * factor).min(1.0);
            let s = if p[3] > 0.0 { na / p[3] } else { 0.0 };
            for c in p.iter_mut() {
                *c *= s;
            }
        }
    }

    // compõe na camada com a opacidade do traço e o modo
    let mode = match preset.mode {
        BrushMode::Normal => BlendMode::Normal,
        BrushMode::Behind => BlendMode::Behind,
        BrushMode::Multiply => BlendMode::Multiply,
        BrushMode::Screen => BlendMode::Screen,
        BrushMode::Overlay => BlendMode::Overlay,
        BrushMode::Add => BlendMode::Add,
        BrushMode::Erase => BlendMode::Normal,
    };
    for y in 0..bh {
        for x in 0..bw {
            let s = buf[y * bw + x];
            if s[3] <= 0.0 {
                continue;
            }
            let i = layer.idx(x as u32 + x0 as u32, y as u32 + y0 as u32);
            if preset.mode == BrushMode::Erase {
                let k = 1.0 - s[3] * preset.opacity;
                for c in layer.data[i].iter_mut() {
                    *c *= k;
                }
            } else {
                blend_pixel(&mut layer.data[i], s, mode, preset.opacity);
            }
        }
    }
    Some(PaintResult { bounds: [x0, y0, x1, y1] })
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

/// Degradê linear ou radial com paradas (t, cor sRGB não pré-multiplicada), opcionalmente limitado por máscara.
pub fn gradient_fill(layer: &mut Image, kind: GradientKind, p0: [f32; 2], p1: [f32; 2], stops: &[(f32, [f32; 4])], mode: BlendMode, opacity: f32) {
    if stops.is_empty() {
        return;
    }
    let (dx, dy) = (p1[0] - p0[0], p1[1] - p0[1]);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    let radius = len2.sqrt();
    let sample = |t: f32| -> [f32; 4] {
        let t = t.clamp(0.0, 1.0);
        if t <= stops[0].0 {
            return stops[0].1;
        }
        for w in stops.windows(2) {
            if t <= w[1].0 {
                let f = (t - w[0].0) / (w[1].0 - w[0].0).max(1e-6);
                let mut c = [0.0; 4];
                for k in 0..4 {
                    c[k] = w[0].1[k] + (w[1].1[k] - w[0].1[k]) * f;
                }
                return c;
            }
        }
        stops[stops.len() - 1].1
    };
    let wdt = layer.width as usize;
    for (i, px) in layer.data.iter_mut().enumerate() {
        let (x, y) = ((i % wdt) as f32 + 0.5, (i / wdt) as f32 + 0.5);
        let t = match kind {
            GradientKind::Linear => ((x - p0[0]) * dx + (y - p0[1]) * dy) / len2,
            GradientKind::Radial => ((x - p0[0]).powi(2) + (y - p0[1]).powi(2)).sqrt() / radius,
        };
        let c = sample(t);
        blend_pixel(px, [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]], mode, opacity);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientKind {
    Linear,
    Radial,
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
        t.texture = Some(BrushTexture { kind: TextureKind::Grain, scale: 2.0, depth: 1.0, mode: TextureMode::Multiply, seed: 1, invert: false });
        let mut img2 = Image::new(200, 100);
        paint_stroke(&mut img2, &t, &line(30), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        let vals: Vec<f32> = (40..80).map(|x| img2.get(x, 50)[3]).collect();
        let mn = vals.iter().cloned().fold(1.0, f32::min);
        let mx = vals.iter().cloned().fold(0.0, f32::max);
        assert!(mx - mn > 0.2, "textura modula o alfa: {mn}..{mx}");
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
}
