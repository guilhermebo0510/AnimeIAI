//! Marca provisória do AnimeIAI (SVG original em `assets/brand/logo.svg`): ícone da janela e diálogo "Sobre".

use crate::icons::{rasterize, IconId};

pub const APP_NAME: &str = "AnimeIAI";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Ícone da janela: o logo rasterizado em 256x256 (RGBA não premultiplicado, como o `IconData` exige).
pub fn window_icon() -> egui::IconData {
    match rasterize(IconId::Logo.svg(), 256) {
        Ok((px, mut data)) => {
            for p in data.chunks_exact_mut(4) {
                let a = p[3] as u32;
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            egui::IconData { rgba: data, width: px, height: px }
        }
        Err(e) => {
            tracing::warn!("ícone da janela inválido: {e}");
            egui::IconData::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_is_a_256_square_with_content() {
        let ic = window_icon();
        assert_eq!((ic.width, ic.height), (256, 256));
        assert_eq!(ic.rgba.len(), 256 * 256 * 4);
        assert!(ic.rgba.chunks_exact(4).filter(|p| p[3] == 255).count() > 256 * 256 / 2);
    }
}
