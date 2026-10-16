//! Catálogo de efeitos. A implementação mora nos módulos `blur`, `light`, `style`, `color` e `lut`; `apply` só
//! despacha e valida parâmetros (valor não finito vira erro, que o compositor registra como aviso).

use crate::color::GradeParams;
use crate::lut::{self, Lut};
use crate::util::{check_finite, for_each_pixel};
use crate::{FxContext, blur, light, style};
use aiai_core::BlobStore;
use aiai_paint::Image;
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
    Glow {
        threshold: f32,
        radius: f32,
        intensity: f32,
        #[serde(default)]
        color: Option<[f32; 3]>,
        /// Combina em tela em vez de soma.
        #[serde(default)]
        screen: bool,
    },
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

    /// Aplica o efeito. `ctx.below` é necessário para `LightWrap`. Parâmetros não finitos viram erro.
    pub fn apply(&self, img: &mut Image, ctx: &FxContext) -> Result<(), String> {
        let name = self.name();
        match self {
            Effect::GaussianBlur { radius } => {
                check_finite(name, &[*radius])?;
                blur::gaussian_blur(img, *radius);
            }
            Effect::DirectionalBlur { angle, length } => {
                check_finite(name, &[*angle, *length])?;
                blur::directional_blur(img, *angle, *length);
            }
            Effect::RadialBlur { center, amount } => {
                check_finite(name, &[center[0], center[1], *amount])?;
                blur::radial_blur(img, *center, *amount);
            }
            Effect::ColorAwareBlur { radius, tolerance } => {
                check_finite(name, &[*radius, *tolerance])?;
                blur::color_aware_blur(img, *radius, *tolerance);
            }
            Effect::Glow { threshold, radius, intensity, color, screen } => {
                check_finite(name, &[*threshold, *radius, *intensity])?;
                light::glow(img, *threshold, *radius, *intensity, *color, *screen);
            }
            Effect::Diffusion { radius, amount, threshold } => {
                check_finite(name, &[*radius, *amount, *threshold])?;
                light::diffusion(img, *radius, *amount, *threshold);
            }
            Effect::Halation { threshold, radius, intensity } => {
                check_finite(name, &[*threshold, *radius, *intensity])?;
                light::halation(img, *threshold, *radius, *intensity);
            }
            Effect::TransmittedLight { radius, intensity, color } => {
                check_finite(name, &[*radius, *intensity, color[0], color[1], color[2]])?;
                light::transmitted_light(img, *radius, *intensity, *color);
            }
            Effect::LightWrap { radius, amount } => {
                check_finite(name, &[*radius, *amount])?;
                let below = ctx.below.ok_or_else(|| "light_wrap precisa do que está composto abaixo (contexto sem 'below')".to_string())?;
                light::light_wrap(img, below, *radius, *amount)?;
            }
            Effect::RimLight { angle, width, color, intensity } => {
                check_finite(name, &[*angle, *width, *intensity, color[0], color[1], color[2]])?;
                light::rim_light(img, *angle, *width, *color, *intensity);
            }
            Effect::Para { from, to, color_from, color_to, mode } => {
                check_finite(name, &[from[0], from[1], to[0], to[1]])?;
                check_finite(name, color_from)?;
                check_finite(name, color_to)?;
                style::para(img, *from, *to, *color_from, *color_to, *mode);
            }
            Effect::ChromaticAberration { amount, center } => {
                check_finite(name, &[*amount])?;
                if let Some(c) = center {
                    check_finite(name, c)?;
                }
                style::chromatic_aberration(img, *amount, *center);
            }
            Effect::Vignette { amount, softness, color } => {
                check_finite(name, &[*amount, *softness])?;
                style::vignette(img, *amount, *softness, *color);
            }
            Effect::Grain { amount, size } => {
                check_finite(name, &[*amount, *size])?;
                style::grain(img, *amount, *size, ctx.seed, ctx.frame);
            }
            Effect::Deband { amount } => {
                check_finite(name, &[*amount])?;
                style::deband(img, *amount);
            }
            Effect::LensFlare { position, intensity, color } => {
                check_finite(name, &[position[0], position[1], *intensity, color[0], color[1], color[2]])?;
                light::lens_flare(img, *position, *intensity, *color);
            }
            Effect::GodRays { center, threshold, length, intensity } => {
                check_finite(name, &[center[0], center[1], *threshold, *length, *intensity])?;
                light::god_rays(img, *center, *threshold, *length, *intensity);
            }
            Effect::DropShadow { offset, blur, color } => {
                check_finite(name, &[offset[0], offset[1], *blur])?;
                check_finite(name, color)?;
                style::drop_shadow(img, *offset, *blur, *color);
            }
            Effect::Outline { width, color } => {
                check_finite(name, &[*width])?;
                check_finite(name, color)?;
                style::outline(img, *width, *color);
            }
            Effect::ColorOverlay { color, mode } => {
                check_finite(name, color)?;
                style::color_overlay(img, *color, *mode);
            }
            Effect::Invert => {
                for_each_pixel(img, |_, _, p| {
                    for c in 0..3 {
                        p[c] = p[3] - p[c];
                    }
                });
            }
            Effect::TwoTone { threshold, dark, light } => {
                check_finite(name, &[*threshold])?;
                style::two_tone(img, *threshold, *dark, *light);
            }
            Effect::Adjust { adjustment } => {
                adjustment.apply_image(img, 1.0, None);
            }
            Effect::Lut { blob, intensity } => {
                check_finite(name, &[*intensity])?;
                let l = lut::load(ctx.blobs, blob)?;
                l.apply_image(img, *intensity);
            }
            Effect::Grade { .. } => {
                let g = self.grade_params().expect("é Grade");
                check_finite(name, &g.to_vec())?;
                g.apply_image(img);
            }
        }
        Ok(())
    }

    /// Parâmetros de `Grade` (None para os outros efeitos).
    pub fn grade_params(&self) -> Option<GradeParams> {
        match self {
            Effect::Grade { lift, gamma, gain, saturation, temperature, tint, contrast, pivot } => Some(GradeParams {
                lift: *lift,
                gamma: *gamma,
                gain: *gain,
                saturation: *saturation,
                temperature: *temperature,
                tint: *tint,
                contrast: *contrast,
                pivot: *pivot,
            }),
            _ => None,
        }
    }

    /// Função de cor pontual (cor reta 0..1 → cor reta) dos efeitos que são só cor: Grade, Adjust, Invert, TwoTone e
    /// Lut. Os demais devolvem erro (dependem de vizinhança, alfa ou contexto). `clamp` limita o Grade a 0..1.
    pub fn pointwise(&self, blobs: &BlobStore, clamp: bool) -> Result<Box<dyn Fn([f32; 3]) -> [f32; 3] + Send + Sync>, String> {
        match self {
            Effect::Grade { .. } => {
                let g = self.grade_params().expect("é Grade");
                // `clamp = false`: sem limite final (ver `GradeParams::apply_rgb_raw`), para cozinhar LUTs
                if clamp { Ok(Box::new(move |c| g.apply_rgb(c))) } else { Ok(Box::new(move |c| g.apply_rgb_raw(c))) }
            }
            Effect::Adjust { adjustment } => {
                let a = adjustment.clone();
                Ok(Box::new(move |c| a.apply_rgb(c)))
            }
            Effect::Invert => Ok(Box::new(|c| [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]])),
            Effect::TwoTone { threshold, dark, light } => {
                let (t, d, l) = (*threshold, *dark, *light);
                Ok(Box::new(move |c| if crate::util::luma(c) >= t { l } else { d }))
            }
            Effect::Lut { blob, intensity } => {
                let lut = lut::load(blobs, blob)?;
                let k = intensity.max(0.0).min(1.0);
                Ok(Box::new(move |c| {
                    let o = lut.sample(c);
                    [c[0] + (o[0] - c[0]) * k, c[1] + (o[1] - c[1]) * k, c[2] + (o[2] - c[2]) * k]
                }))
            }
            other => Err(format!("efeito '{}' não é uma função de cor pontual (não pode virar LUT)", other.name())),
        }
    }
}

/// Cozinha uma cadeia de efeitos de cor pontuais numa LUT 3D de lado `size` (2..=65) para exportar como `.cube`
/// (`Lut::to_cube`). Valores baixados podem passar de 0..1 (o aplicador limita).
pub fn bake_lut(effects: &[Effect], size: usize, title: &str, blobs: &BlobStore) -> Result<Lut, String> {
    // Os estágios limitam a 0..1 como na aplicação em imagem, exceto o último que não seja Invert: limites no meio
    // criam quinas na tabela e o `Invert` final comuta com o limite (clamp(1−x) = 1−clamp(x)), então o aplicador
    // limita depois de interpolar.
    let last = effects.iter().rposition(|e| !matches!(e, Effect::Invert));
    let fns: Vec<_> = effects.iter().enumerate().map(|(i, e)| e.pointwise(blobs, Some(i) != last)).collect::<Result<_, _>>()?;
    // Os valores podem sair de 0..1 (Grade sem limite final); `Lut::apply_image` limita depois de interpolar.
    Ok(Lut::bake(size, title, |c| {
        let mut v = c;
        for f in &fns {
            v = f(v);
        }
        v
    }))
}
