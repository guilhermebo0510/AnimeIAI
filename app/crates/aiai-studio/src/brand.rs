//! Marca do AnimeIAI: SVG original desenhado à mão em `assets/brand/` — `logo.svg` (escuro, padrão e ícone da janela),
//! `logo-light.svg` (tema claro) e `logo-small.svg` (16 a 32 px). Um quadro de animação com perfurações de filme e um "A"
//! em pincelada no laranja do acento. PNGs de 16 a 512 px: `brand_pngs` (teste ignorado); splash: `tools/brand/splash.py`.

use crate::icons::{rasterize, IconId};

pub const APP_NAME: &str = "AnimeIAI";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A marca certa para o tema (o fundo do quadrado acompanha o tema da interface).
pub fn logo_for(theme: crate::theme::ThemeKind) -> IconId {
    match theme {
        crate::theme::ThemeKind::Dark => IconId::Logo,
        crate::theme::ThemeKind::Light => IconId::LogoLight,
    }
}

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

    /// PNGs da marca para entrega/apresentação: `AIAI_BRAND_OUT=<pasta> cargo test -p aiai-studio brand_pngs -- --ignored`.
    /// `logo-<tema>-<px>.png` de 48 a 512 e `logo-small-<px>.png` de 16 a 32 (abaixo de 48 o detalhe vira ruído).
    #[test]
    #[ignore]
    fn brand_pngs() {
        let Some(out) = std::env::var_os("AIAI_BRAND_OUT") else { return };
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        let mut jobs: Vec<(IconId, &str, u32)> = Vec::new();
        for px in [48, 64, 128, 256, 512] {
            jobs.push((IconId::Logo, "dark", px));
            jobs.push((IconId::LogoLight, "light", px));
        }
        for px in [16, 24, 32] {
            jobs.push((IconId::LogoSmall, "small", px));
        }
        for (id, tag, px) in jobs {
            let (n, mut data) = rasterize(id.svg(), px).unwrap();
            assert_eq!(n, px);
            // tiny-skia entrega premultiplicado; PNG é direto
            for p in data.chunks_exact_mut(4) {
                let a = p[3] as u32;
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            image::save_buffer(out.join(format!("logo-{tag}-{px}.png")), &data, px, px, image::ColorType::Rgba8).unwrap();
        }
    }
}
