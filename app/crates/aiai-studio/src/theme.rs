//! Tema claro/escuro (sóbrio, referência: Blender e DaVinci Resolve; um único acento quente) e fontes embutidas.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Espaço entre painéis (a moldura) e raio dos cantos.
pub const CARD_GAP: f32 = 5.0;
pub const CARD_RADIUS: u8 = 6;
/// Altura do cabeçalho de painel.
pub const HEADER_H: f32 = 24.0;
/// Largura/altura mínima de um painel do dock.
pub const PANEL_MIN: f32 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeKind {
    #[default]
    Dark,
    Light,
}

impl ThemeKind {
    pub fn key(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
        }
    }

    pub fn from_key(key: &str) -> Option<ThemeKind> {
        match key {
            "dark" | "escuro" => Some(ThemeKind::Dark),
            "light" | "claro" => Some(ThemeKind::Light),
            _ => None,
        }
    }

    pub fn tokens(self) -> Tokens {
        match self {
            ThemeKind::Dark => Tokens::dark(),
            ThemeKind::Light => Tokens::light(),
        }
    }
}

/// Tokens de cor de um tema. Todo desenho próprio do app usa estes valores (nada de cor solta nos painéis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// Fundo "entre os painéis" (a moldura da janela).
    pub bg_frame: Color32,
    /// Cabeçalhos de painel, barras (menu, workspaces, status).
    pub bg_header: Color32,
    /// Corpo dos painéis.
    pub bg_panel: Color32,
    pub bg_widget: Color32,
    pub bg_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    /// Acento único (quente).
    pub accent: Color32,
    /// Preenchimento de seleção (acento atenuado, legível sob `text`).
    pub accent_dim: Color32,
    /// Mesa de trabalho do canvas.
    pub canvas_bg: Color32,
    /// Interior do quadro da câmera.
    pub canvas_frame_fill: Color32,
    pub canvas_frame_stroke: Color32,
    /// Estados de etapa da produção (fundo de etiqueta, com `text` por cima): em andamento, para revisão, aprovado, retake.
    pub st_progress: Color32,
    pub st_review: Color32,
    pub st_ok: Color32,
    pub st_retake: Color32,
    /// Erro e "vai apagar" (traços que a borracha pega, falha do render no canvas): como texto e como traço.
    pub danger: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Tokens {
    pub const fn dark() -> Tokens {
        Tokens {
            bg_frame: rgb(0x0f, 0x10, 0x12),
            bg_header: rgb(0x18, 0x19, 0x1c),
            bg_panel: rgb(0x22, 0x23, 0x27),
            bg_widget: rgb(0x2d, 0x2e, 0x34),
            bg_hover: rgb(0x39, 0x3b, 0x42),
            border: rgb(0x3a, 0x3c, 0x43),
            text: rgb(0xdd, 0xde, 0xe2),
            text_dim: rgb(0xa6, 0xa8, 0xb1),
            accent: rgb(0xf0, 0x98, 0x3c),
            accent_dim: rgb(0x6e, 0x45, 0x1d),
            canvas_bg: rgb(0x2a, 0x2b, 0x30),
            canvas_frame_fill: rgb(0x31, 0x32, 0x38),
            canvas_frame_stroke: rgb(0xb4, 0xb6, 0xbf),
            st_progress: rgb(0x2b, 0x4a, 0x74),
            st_review: rgb(0x65, 0x4e, 0x14),
            st_ok: rgb(0x2a, 0x58, 0x37),
            st_retake: rgb(0x77, 0x2a, 0x2a),
            danger: rgb(0xf0, 0x7a, 0x66),
        }
    }

    pub const fn light() -> Tokens {
        Tokens {
            bg_frame: rgb(0xb9, 0xbb, 0xc1),
            bg_header: rgb(0xde, 0xdf, 0xe3),
            bg_panel: rgb(0xec, 0xed, 0xef),
            bg_widget: rgb(0xdb, 0xdc, 0xe1),
            bg_hover: rgb(0xcc, 0xce, 0xd4),
            border: rgb(0xb0, 0xb2, 0xb9),
            text: rgb(0x1f, 0x20, 0x24),
            text_dim: rgb(0x45, 0x48, 0x50),
            accent: rgb(0x91, 0x47, 0x09),
            accent_dim: rgb(0xf6, 0xcf, 0xa6),
            canvas_bg: rgb(0xa9, 0xab, 0xb1),
            canvas_frame_fill: rgb(0xb6, 0xb8, 0xbe),
            canvas_frame_stroke: rgb(0x3c, 0x3e, 0x46),
            st_progress: rgb(0xc4, 0xd8, 0xf2),
            st_review: rgb(0xf0, 0xdc, 0x9a),
            st_ok: rgb(0xc2, 0xe4, 0xca),
            st_retake: rgb(0xf2, 0xc2, 0xc2),
            danger: rgb(0x7a, 0x16, 0x0e),
        }
    }
}

/// Luminância relativa WCAG de uma cor sRGB opaca.
pub fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Razão de contraste WCAG entre duas cores (1..21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Aplica o tema ao egui (Visuals + Style). Troca em tempo real: chamar de novo com o outro `ThemeKind`.
pub fn apply(ctx: &egui::Context, kind: ThemeKind) {
    let t = kind.tokens();
    let mut v = match kind {
        ThemeKind::Dark => egui::Visuals::dark(),
        ThemeKind::Light => egui::Visuals::light(),
    };
    v.panel_fill = t.bg_panel;
    v.window_fill = t.bg_panel;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.extreme_bg_color = t.bg_header;
    v.faint_bg_color = t.bg_header;
    v.code_bg_color = t.bg_widget;
    v.override_text_color = Some(t.text);
    v.selection.bg_fill = t.accent_dim;
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.popup_shadow = egui::Shadow::NONE;
    v.window_shadow = egui::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(90) };
    let r = CornerRadius::same(4);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = t.bg_panel;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    v.widgets.inactive.bg_fill = t.bg_widget;
    v.widgets.inactive.weak_bg_fill = t.bg_widget;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.hovered.bg_fill = t.bg_hover;
    v.widgets.hovered.weak_bg_fill = t.bg_hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, t.border);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5, t.text);
    v.widgets.active.bg_fill = t.accent_dim;
    v.widgets.active.weak_bg_fill = t.accent_dim;
    v.widgets.active.bg_stroke = Stroke::new(1.0, t.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.5, t.text);
    v.widgets.open.bg_fill = t.bg_hover;
    v.widgets.open.weak_bg_fill = t.bg_hover;
    v.widgets.open.fg_stroke = Stroke::new(1.0, t.text);
    v.window_corner_radius = CornerRadius::same(6);
    v.menu_corner_radius = CornerRadius::same(6);

    // O app não segue o tema do sistema: fixa a preferência e o slot correspondente.
    match kind {
        ThemeKind::Dark => {
            ctx.set_theme(egui::ThemePreference::Dark);
            ctx.set_visuals_of(egui::Theme::Dark, v);
        }
        ThemeKind::Light => {
            ctx.set_theme(egui::ThemePreference::Light);
            ctx.set_visuals_of(egui::Theme::Light, v);
        }
    }
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.menu_margin = egui::Margin::symmetric(6, 5);
        style.text_styles.insert(egui::TextStyle::Body, egui::FontId::new(13.0, FontFamily::Proportional));
        style.text_styles.insert(egui::TextStyle::Button, egui::FontId::new(13.0, FontFamily::Proportional));
        style.text_styles.insert(egui::TextStyle::Small, egui::FontId::new(11.0, FontFamily::Proportional));
        style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::new(18.0, FontFamily::Proportional));
        style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::new(12.5, FontFamily::Monospace));
    });
}

const NOTO_SANS: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
const NOTO_SANS_JP: &[u8] = include_bytes!("../assets/fonts/NotoSansJP-Regular.otf");

/// Fontes: Noto Sans (latim) + Noto Sans JP (japonês) como proporcional; monoespaçada padrão do egui (Hack) com Noto Sans JP de reserva.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert("noto-sans".to_owned(), Arc::new(FontData::from_static(NOTO_SANS)));
    fonts.font_data.insert("noto-sans-jp".to_owned(), Arc::new(FontData::from_static(NOTO_SANS_JP)));
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "noto-sans".to_owned());
    prop.insert(1, "noto-sans-jp".to_owned());
    fonts.families.entry(FontFamily::Monospace).or_default().push("noto-sans-jp".to_owned());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_keys_round_trip() {
        for k in [ThemeKind::Dark, ThemeKind::Light] {
            assert_eq!(ThemeKind::from_key(k.key()), Some(k));
        }
        assert_eq!(ThemeKind::from_key("x"), None);
    }

    #[test]
    fn tokens_are_readable_in_both_themes() {
        for k in [ThemeKind::Dark, ThemeKind::Light] {
            let t = k.tokens();
            for (name, bg) in [("panel", t.bg_panel), ("header", t.bg_header), ("widget", t.bg_widget), ("hover", t.bg_hover), ("accent_dim", t.accent_dim)] {
                assert!(contrast(t.text, bg) >= 4.5, "{k:?}: texto sobre {name} = {:.2}", contrast(t.text, bg));
            }
            // AA (4,5:1) em todo par texto × fundo que a UI desenha: atenuado nas abas de workspace (moldura), em hover e
            // nos widgets; acento como TEXTO (barra de status, rótulos ativos) e texto sobre as etiquetas de etapa
            for (name, bg) in [("panel", t.bg_panel), ("header", t.bg_header), ("widget", t.bg_widget), ("hover", t.bg_hover), ("frame", t.bg_frame)] {
                assert!(contrast(t.text_dim, bg) >= 4.5, "{k:?}: texto atenuado sobre {name} = {:.2}", contrast(t.text_dim, bg));
            }
            for (name, bg) in [("panel", t.bg_panel), ("header", t.bg_header), ("widget", t.bg_widget)] {
                assert!(contrast(t.accent, bg) >= 4.5, "{k:?}: acento sobre {name} = {:.2}", contrast(t.accent, bg));
            }
            for (name, bg) in [("panel", t.bg_panel), ("header", t.bg_header), ("canvas", t.canvas_bg)] {
                assert!(contrast(t.danger, bg) >= 4.5, "{k:?}: erro sobre {name} = {:.2}", contrast(t.danger, bg));
            }
            for (name, bg) in [("andamento", t.st_progress), ("revisão", t.st_review), ("aprovado", t.st_ok), ("retake", t.st_retake)] {
                assert!(contrast(t.text, bg) >= 4.5, "{k:?}: texto sobre etapa {name} = {:.2}", contrast(t.text, bg));
            }
            assert!(contrast(t.canvas_frame_stroke, t.canvas_bg) >= 3.0, "{k:?}: contorno do quadro = {:.2}", contrast(t.canvas_frame_stroke, t.canvas_bg));
            // hierarquia de profundidade: o canvas/moldura se distinguem do painel
            assert_ne!(t.bg_frame, t.bg_panel);
            assert_ne!(t.canvas_bg, t.canvas_frame_fill);
            for c in [t.bg_frame, t.bg_header, t.bg_panel, t.text, t.accent] {
                assert_eq!(c.a(), 255);
            }
        }
    }

    #[test]
    fn dark_is_darker_than_light() {
        assert!(luminance(Tokens::dark().bg_panel) < luminance(Tokens::light().bg_panel));
    }

    #[test]
    fn embedded_fonts_are_real_font_files() {
        assert!(NOTO_SANS.len() > 100_000 && NOTO_SANS_JP.len() > 1_000_000);
        // TTF: 0x00010000; OTF/CFF: "OTTO"
        assert_eq!(&NOTO_SANS[..4], &[0, 1, 0, 0]);
        assert_eq!(&NOTO_SANS_JP[..4], b"OTTO");
    }
}
