//! Catálogo de efeitos. A implementação de cada efeito mora em `apply`; os ainda não implementados devolvem erro
//! explícito (vira aviso no render), nunca uma imagem silenciosamente inalterada.

use crate::FxContext;
use aiai_paint::Image;
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// Desfoque gaussiano (raio ≈ 3σ, px).
    GaussianBlur { radius: f32 },
    /// Desfoque de movimento numa direção (graus, horário a partir de +x) e comprimento (px).
    DirectionalBlur { angle: f32, length: f32 },
    /// Desfoque radial/zoom a partir de um centro (px); `amount` 0..1.
    RadialBlur { center: [f32; 2], amount: f32 },
    /// Desfoque que não mistura regiões de cor diferentes (cel): só mistura vizinhos com diferença ≤ `tolerance`.
    ColorAwareBlur { radius: f32, tolerance: f32 },
    /// Brilho: realces acima de `threshold` borrados e somados (cor opcional).
    Glow { threshold: f32, radius: f32, intensity: f32, #[serde(default)] color: Option<[f32; 3]> },
    /// Difusão (soft focus de anime): versão borrada misturada em tela/luz suave acima de um limiar.
    Diffusion { radius: f32, amount: f32, #[serde(default)] threshold: f32 },
    /// Halação: glow avermelhado em torno de realces fortes.
    Halation { threshold: f32, radius: f32, intensity: f32 },
    /// Luz transmitida (透過光): a forma da camada vira fonte de luz forte com glow em camadas.
    TransmittedLight { radius: f32, intensity: f32, color: [f32; 3] },
    /// Light wrap: luz do fundo (`below`) vaza para as bordas da camada.
    LightWrap { radius: f32, amount: f32 },
    /// Luz de recorte 2D: borda do alfa na direção `angle` (graus) pintada com `color`.
    RimLight { angle: f32, width: f32, color: [f32; 3], intensity: f32 },
    /// "Para" (パラ): degradê de cor sobre a camada (recortado pelo alfa), em modo de mistura.
    Para { from: [f32; 2], to: [f32; 2], color_from: [f32; 4], color_to: [f32; 4], #[serde(default)] mode: aiai_core::BlendMode },
    ChromaticAberration { amount: f32, #[serde(default)] center: Option<[f32; 2]> },
    Vignette { amount: f32, softness: f32, #[serde(default)] color: Option<[f32; 3]> },
    /// Grão de filme (semente varia por quadro).
    Grain { amount: f32, size: f32 },
    /// Dither leve contra faixas em degradês.
    Deband { amount: f32 },
    LensFlare { position: [f32; 2], intensity: f32, color: [f32; 3] },
    /// Raios de luz a partir de um centro (realces acima do limiar, desfoque radial).
    GodRays { center: [f32; 2], threshold: f32, length: f32, intensity: f32 },
    DropShadow { offset: [f32; 2], blur: f32, color: [f32; 4] },
    /// Contorno em volta do alfa.
    Outline { width: f32, color: [f32; 4] },
    ColorOverlay { color: [f32; 4], #[serde(default)] mode: aiai_core::BlendMode },
    Invert,
    /// Impact frame / duas cores por limiar de luminância.
    TwoTone { threshold: f32, dark: [f32; 3], light: [f32; 3] },
    /// Ajuste de cor (níveis, curvas, HSL...).
    Adjust { adjustment: aiai_paint::adjust::Adjustment },
    /// LUT 3D (.cube guardado em blob).
    Lut { blob: aiai_core::BlobHash, #[serde(default = "one")] intensity: f32 },
    /// Correção primária (rodas de cor): lift/gamma/gain RGB, saturação, temperatura, contraste com pivô.
    Grade {
        #[serde(default)]
        lift: [f32; 3],
        #[serde(default = "unit3")]
        gamma: [f32; 3],
        #[serde(default = "unit3")]
        gain: [f32; 3],
        #[serde(default = "one")]
        saturation: f32,
        #[serde(default)]
        temperature: f32,
        #[serde(default)]
        tint: f32,
        #[serde(default = "one")]
        contrast: f32,
        #[serde(default = "half")]
        pivot: f32,
    },
}

fn one() -> f32 {
    1.0
}
fn half() -> f32 {
    0.5
}
fn unit3() -> [f32; 3] {
    [1.0; 3]
}

impl Effect {
    pub fn name(&self) -> &'static str {
        match self {
            Effect::GaussianBlur { .. } => "gaussian_blur",
            Effect::DirectionalBlur { .. } => "directional_blur",
            Effect::RadialBlur { .. } => "radial_blur",
            Effect::ColorAwareBlur { .. } => "color_aware_blur",
            Effect::Glow { .. } => "glow",
            Effect::Diffusion { .. } => "diffusion",
            Effect::Halation { .. } => "halation",
            Effect::TransmittedLight { .. } => "transmitted_light",
            Effect::LightWrap { .. } => "light_wrap",
            Effect::RimLight { .. } => "rim_light",
            Effect::Para { .. } => "para",
            Effect::ChromaticAberration { .. } => "chromatic_aberration",
            Effect::Vignette { .. } => "vignette",
            Effect::Grain { .. } => "grain",
            Effect::Deband { .. } => "deband",
            Effect::LensFlare { .. } => "lens_flare",
            Effect::GodRays { .. } => "god_rays",
            Effect::DropShadow { .. } => "drop_shadow",
            Effect::Outline { .. } => "outline",
            Effect::ColorOverlay { .. } => "color_overlay",
            Effect::Invert => "invert",
            Effect::TwoTone { .. } => "two_tone",
            Effect::Adjust { .. } => "adjust",
            Effect::Lut { .. } => "lut",
            Effect::Grade { .. } => "grade",
        }
    }

    pub fn apply(&self, img: &mut Image, _ctx: &FxContext) -> Result<(), String> {
        match self {
            Effect::Invert => {
                img.data.par_iter_mut().for_each(|p| {
                    for c in 0..3 {
                        p[c] = p[3] - p[c];
                    }
                });
                Ok(())
            }
            Effect::Adjust { adjustment } => {
                adjustment.apply_image(img, 1.0, None);
                Ok(())
            }
            other => Err(format!("efeito '{}' ainda não implementado", other.name())),
        }
    }
}
