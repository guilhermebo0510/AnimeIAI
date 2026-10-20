//! Estilos de texto guardáveis (`.aiaistyle` em `Documentos\AnimeIAI\Estilos`): a tinta, os efeitos, a distorção e a
//! fonte de uma camada de texto, sem o conteúdo nem a posição. As medidas em px foram desenhadas num corpo de
//! referência (`ref_size`) e escalam com o tamanho do texto que recebe o estilo.
//!
//! Os estilos prontos (`builtin_styles`) são desenhados aqui, procedurais: títulos de anime comuns (ouro com contorno
//! duplo, mahō shōjo, neon, metal, extrusão pop, terror, comédia, pôr do sol). Nenhum arquivo de fora.
//!
//! Quem chama decide se pode ler/gravar arquivos (o app desliga tudo com `--no-session`).

use crate::text_layer::*;
use aiai_core::{ColorRef, Gradient, GradientExtend, GradientShape, GradientSpace, GradientStop, Paint, Rgba};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STYLE_EXT: &str = "aiaistyle";
pub const STYLE_VERSION: u32 = 1;

/// Estilo de texto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct TextStyle {
    pub v: u32,
    /// Marca do arquivo: sempre "aiai_text_style".
    pub kind: String,
    pub name: String,
    /// Corpo (px) em que as medidas foram pensadas.
    pub ref_size: f32,
    pub font: Option<String>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub tracking: Option<f32>,
    pub color: Option<ColorRef>,
    pub fill: Option<Paint>,
    pub stroke: Option<TextStroke>,
    pub shadow: Option<TextShadow>,
    pub effects: Vec<TextEffect>,
    pub warp: Option<TextWarp>,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            v: STYLE_VERSION,
            kind: "aiai_text_style".into(),
            name: String::new(),
            ref_size: 100.0,
            font: None,
            bold: None,
            italic: None,
            tracking: None,
            color: None,
            fill: None,
            stroke: None,
            shadow: None,
            effects: Vec::new(),
            warp: None,
        }
    }
}

impl TextStyle {
    /// O estilo de uma camada (o corpo dela vira a referência).
    pub fn from_params(name: &str, p: &TextParams) -> TextStyle {
        TextStyle {
            name: name.trim().to_owned(),
            ref_size: p.size,
            font: Some(p.font.clone()),
            bold: Some(p.bold),
            italic: Some(p.italic),
            tracking: Some(p.tracking),
            color: Some(p.color.clone()),
            fill: p.fill.clone(),
            stroke: p.stroke.clone(),
            shadow: p.shadow.clone(),
            effects: p.effects.clone(),
            warp: p.warp.clone(),
            ..Default::default()
        }
    }

    /// Aplica o estilo (escala as medidas para o corpo de `p`). O que o estilo não diz (fonte `None`...) fica.
    pub fn apply(&self, p: &mut TextParams) {
        let k = if self.ref_size > 0.0 && self.ref_size.is_finite() { p.size / self.ref_size } else { 1.0 };
        if let Some(f) = &self.font {
            p.font = f.clone();
        }
        if let Some(b) = self.bold {
            p.bold = b;
        }
        if let Some(i) = self.italic {
            p.italic = i;
        }
        if let Some(t) = self.tracking {
            p.tracking = t;
        }
        if let Some(c) = &self.color {
            p.color = c.clone();
        }
        p.fill = self.fill.clone();
        p.stroke = self.stroke.clone().map(|mut s| {
            s.width *= k;
            s
        });
        p.shadow = self.shadow.clone().map(|mut s| {
            s.offset = [s.offset[0] * k, s.offset[1] * k];
            s.blur *= k;
            s
        });
        p.effects = self
            .effects
            .iter()
            .cloned()
            .map(|mut e| {
                e.scale_px(k);
                e
            })
            .collect();
        p.warp = self.warp.clone();
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Result<TextStyle, String> {
        let st: TextStyle = serde_json::from_str(s).map_err(|e| format!("estilo de texto inválido: {e}"))?;
        if st.kind != "aiai_text_style" {
            return Err("o arquivo não é um estilo de texto do AnimeIAI".into());
        }
        Ok(st)
    }
}

/// `Documentos\AnimeIAI\Estilos`.
pub fn styles_dir() -> Option<PathBuf> {
    crate::userdirs::animeiai_dir().map(|d| d.join("Estilos"))
}

/// Nome de arquivo seguro a partir do nome do estilo.
pub fn file_name_for(name: &str) -> String {
    let base: String = name.trim().chars().map(|c| if c.is_alphanumeric() || " -_()".contains(c) { c } else { '_' }).collect();
    let base = base.trim().trim_matches('.').to_owned();
    format!("{}.{STYLE_EXT}", if base.is_empty() { "estilo".to_owned() } else { base })
}

/// Estilos da pasta (os ilegíveis ficam de fora), em ordem de nome.
pub fn list_styles(dir: &Path) -> Vec<(PathBuf, TextStyle)> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()).is_none_or(|x| !x.eq_ignore_ascii_case(STYLE_EXT)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        if let Ok(mut st) = TextStyle::from_json(&text) {
            if st.name.trim().is_empty() {
                st.name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_owned();
            }
            out.push((path, st));
        }
    }
    out.sort_by_key(|(_, s)| s.name.to_lowercase());
    out
}

/// Grava o estilo na pasta (cria a pasta; sobrescreve o de mesmo nome).
pub fn save_style(dir: &Path, st: &TextStyle) -> Result<PathBuf, String> {
    if st.name.trim().is_empty() {
        return Err("dê um nome ao estilo".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(file_name_for(&st.name));
    std::fs::write(&path, st.to_json()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

pub fn delete_style(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))
}

// ------------------------------------------------------------------------------------------------ prontos

fn hex(s: &str) -> [f32; 4] {
    Rgba::from_hex(s).map(|c| c.0).unwrap_or([0.0, 0.0, 0.0, 1.0])
}

fn col(s: &str) -> ColorRef {
    ColorRef::Direct { rgba: Rgba(hex(s)) }
}

fn grad(cols: &[&str], angle: f32, space: GradientSpace) -> Gradient {
    Gradient { space, ..Gradient::from_colors(&cols.iter().map(|c| hex(c)).collect::<Vec<_>>(), angle) }
}

fn fx(kind: TextEffectKind) -> TextEffect {
    TextEffect::new(kind)
}

fn stroke(width: f32, paint: Paint) -> TextEffect {
    fx(TextEffectKind::Stroke(FxStroke { width, position: StrokePosition::Outside, paint }))
}

/// Estilos prontos (títulos de anime), desenhados no corpo 100 px.
pub fn builtin_styles() -> Vec<TextStyle> {
    let base = |name: &str| TextStyle { name: name.into(), ref_size: 100.0, bold: Some(true), ..Default::default() };
    vec![
        TextStyle {
            fill: Some(Paint::Gradient { gradient: grad(&["#fff8c4", "#ffd23f", "#ff8a00"], 90.0, GradientSpace::Oklab) }),
            effects: vec![
                stroke(5.0, Paint::solid([1.0, 1.0, 1.0, 1.0])),
                stroke(13.0, Paint::solid(hex("#1d1442"))),
                fx(TextEffectKind::DropShadow(FxShadow { offset: [5.0, 8.0], blur: 4.0, spread: 0.0, noise: 0.0, color: col("#000000a0") })),
                fx(TextEffectKind::BevelEmboss(FxBevel { size: 4.0, depth: 1.2, highlight_opacity: 0.55, shadow_opacity: 0.35, ..FxBevel::default() })),
            ],
            warp: Some(TextWarp { style: WarpStyle::Arc, bend: 0.22, ..Default::default() }),
            ..base("Shōnen — ouro, contorno duplo")
        },
        TextStyle {
            fill: Some(Paint::Gradient { gradient: grad(&["#ff8ad8", "#c59bff", "#7fe3ff"], 90.0, GradientSpace::Oklab) }),
            effects: vec![
                stroke(6.0, Paint::solid([1.0, 1.0, 1.0, 1.0])),
                fx(TextEffectKind::OuterGlow(FxGlow { size: 22.0, spread: 0.1, paint: Paint::solid(hex("#ff9ef0d8")), noise: 0.0, from_center: false })),
                TextEffect { opacity: 0.35, ..fx(TextEffectKind::Satin(FxSatin { color: col("#ffffff"), angle: 30.0, distance: 9.0, size: 12.0, invert: false })) },
            ],
            warp: Some(TextWarp { style: WarpStyle::Arch, bend: 0.18, ..Default::default() }),
            ..base("Mahō shōjo — pastel com brilho")
        },
        TextStyle {
            fill: Some(Paint::solid(hex("#effeff"))),
            effects: vec![
                fx(TextEffectKind::Stroke(FxStroke { width: 2.5, position: StrokePosition::Center, paint: Paint::solid(hex("#9ff8ff")) })),
                fx(TextEffectKind::InnerGlow(FxGlow { size: 9.0, spread: 0.0, paint: Paint::solid(hex("#00e5ffe0")), noise: 0.0, from_center: false })),
                TextEffect { blend: aiai_core::BlendMode::Screen, ..fx(TextEffectKind::OuterGlow(FxGlow { size: 28.0, spread: 0.05, paint: Paint::Gradient { gradient: grad(&["#00f0ff", "#ff00d400"], 0.0, GradientSpace::Linear) }, noise: 0.0, from_center: false })) },
            ],
            ..base("Neon")
        },
        TextStyle {
            fill: Some(Paint::Gradient {
                gradient: Gradient {
                    extend: GradientExtend::Reflect,
                    scale: 0.55,
                    center: [0.5, 0.0],
                    ..grad(&["#ffffff", "#c9ced6", "#6d7480", "#e3e7ec"], 90.0, GradientSpace::Srgb)
                },
            }),
            effects: vec![
                fx(TextEffectKind::BevelEmboss(FxBevel { size: 6.0, depth: 2.0, soften: 1.0, ..FxBevel::default() })),
                stroke(4.0, Paint::solid(hex("#101216"))),
                fx(TextEffectKind::LongShadow(FxLongShadow { angle: 60.0, length: 18.0, paint: Paint::Gradient { gradient: grad(&["#454b57", "#0d0f14"], 0.0, GradientSpace::Linear) }, fade: false })),
            ],
            ..base("Mecha — metal")
        },
        TextStyle {
            fill: Some(Paint::solid(hex("#ffe14a"))),
            effects: vec![
                stroke(4.0, Paint::solid(hex("#2b1b5a"))),
                fx(TextEffectKind::LongShadow(FxLongShadow { angle: 45.0, length: 30.0, paint: Paint::Gradient { gradient: grad(&["#ff5a8a", "#7a1f6b"], 0.0, GradientSpace::Oklab) }, fade: false })),
                TextEffect { opacity: 0.6, ..fx(TextEffectKind::DropShadow(FxShadow { offset: [10.0, 12.0], blur: 10.0, spread: 0.0, noise: 0.0, color: col("#000000") })) },
            ],
            ..base("Pop 3D — extrusão")
        },
        TextStyle {
            fill: Some(Paint::Gradient { gradient: grad(&["#ff3030", "#5a0000"], 90.0, GradientSpace::Linear) }),
            effects: vec![
                fx(TextEffectKind::InnerShadow(FxShadow { offset: [0.0, 4.0], blur: 6.0, spread: 0.0, noise: 0.0, color: col("#000000c8") })),
                fx(TextEffectKind::DropShadow(FxShadow { offset: [0.0, 7.0], blur: 10.0, spread: 2.0, noise: 0.6, color: col("#2a0000e0") })),
                stroke(2.0, Paint::solid(hex("#120000"))),
            ],
            warp: Some(TextWarp { style: WarpStyle::Wave, bend: 0.12, ..Default::default() }),
            ..base("Terror")
        },
        TextStyle {
            fill: Some(Paint::solid([1.0, 1.0, 1.0, 1.0])),
            effects: vec![
                stroke(7.0, Paint::solid(hex("#ff3d7f"))),
                stroke(14.0, Paint::solid(hex("#222222"))),
                fx(TextEffectKind::DropShadow(FxShadow { offset: [8.0, 8.0], blur: 0.0, spread: 0.0, noise: 0.0, color: col("#222222") })),
            ],
            warp: Some(TextWarp { style: WarpStyle::Bulge, bend: 0.4, ..Default::default() }),
            ..base("Comédia — inchado")
        },
        TextStyle {
            fill: Some(Paint::Gradient {
                gradient: Gradient {
                    stops: vec![GradientStop::direct(0.0, hex("#ffd36b")), GradientStop::direct(0.45, hex("#ff7a3d")), GradientStop::direct(1.0, hex("#a8325f"))],
                    angle: 90.0,
                    space: GradientSpace::Oklab,
                    ..Default::default()
                },
            }),
            effects: vec![
                fx(TextEffectKind::OuterGlow(FxGlow { size: 18.0, spread: 0.0, paint: Paint::solid(hex("#ffb36bb0")), noise: 0.0, from_center: false })),
                TextEffect { opacity: 0.5, ..fx(TextEffectKind::GradientOverlay(FxGradientOverlay { gradient: Gradient { shape: GradientShape::Radial, ..grad(&["#ffffff60", "#ffffff00"], 0.0, GradientSpace::Linear) } })) },
            ],
            warp: Some(TextWarp { style: WarpStyle::Arch, bend: 0.3, ..Default::default() }),
            ..base("Pôr do sol — encerramento")
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn style_round_trips_through_a_file_and_scales_with_the_size() {
        let dir = std::env::temp_dir().join(format!("aiai_styles_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let st = builtin_styles().remove(0);
        let path = save_style(&dir, &st).unwrap();
        assert!(path.extension().unwrap() == STYLE_EXT);
        let list = list_styles(&dir);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, st);
        // aplicar num texto de 50 px: as medidas caem pela metade
        let mut p = TextParams { content: "Título".into(), size: 50.0, ..Default::default() };
        st.apply(&mut p);
        let TextEffectKind::Stroke(s) = &p.effects[1].kind else { panic!() };
        assert!((s.width - 6.5).abs() < 1e-4, "13 px no corpo 100 → 6,5 no corpo 50");
        assert!(p.fill.is_some() && p.warp.is_some() && p.bold);
        // de volta para estilo e reaplicado no mesmo tamanho: igual
        let back = TextStyle::from_params("x", &p);
        let mut q = TextParams { content: "Título".into(), size: 50.0, ..Default::default() };
        back.apply(&mut q);
        assert_eq!((q.effects.clone(), q.fill.clone(), q.warp.clone()), (p.effects.clone(), p.fill.clone(), p.warp.clone()));
        delete_style(&path).unwrap();
        assert!(list_styles(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(TextStyle::from_json("{\"kind\": \"outra\"}").is_err());
    }

    #[test]
    fn builtins_are_valid_and_survive_serde() {
        let all = builtin_styles();
        assert!(all.len() >= 6);
        for st in &all {
            let back = TextStyle::from_json(&st.to_json()).unwrap();
            assert_eq!(&back, st, "{}", st.name);
            for e in &st.effects {
                if let TextEffectKind::Stroke(s) = &e.kind {
                    assert!(s.width > 0.0);
                }
            }
        }
        assert_eq!(file_name_for("Shōnen / ouro"), "Shōnen _ ouro.aiaistyle");
    }
}
