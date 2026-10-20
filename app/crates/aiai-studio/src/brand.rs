//! Marca do AnimeIAI: SVG original desenhado à mão em `assets/brand/` (gerado por `tools/brand/make_svgs.py`).
//!
//! Um quadro de animação com perfurações de filme e um "A" em pincelada no laranja do acento, com a cópia fantasma do
//! quadro anterior (onion skin). Arquivos: `logo*.svg` (marca; `-dark` para interface escura, `-light` para clara,
//! `-small` de 16 a 32 px, `-mono` em `currentColor`), `logo-wordmark*.svg` (marca + "AnimeIAI" em traçados próprios),
//! `hinted/` (16, 20, 24 e 32 px na grade de pixels), `aiai.ico` (o ícone do exe, embutido por `build.rs`) e `png/`.
//!
//! O ícone da janela e da barra de tarefas em execução é definido aqui: `with_icon` do eframe só chega como ícone
//! pequeno (o winit não define o grande), então `init_window_icons` manda os dois tamanhos certos (`WM_SETICON`),
//! desenhados na grade de pixels, na variante que contrasta com a barra de tarefas do Windows (quadrado escuro sobre
//! barra clara, quadrado claro sobre barra escura). O ícone do exe (Explorer, atalho, Iniciar, botão fixado) é sempre
//! o quadrado escuro: o recurso embutido não muda com o tema.

use crate::icons::{rasterize, IconId};
use crate::theme::ThemeKind;
use resvg::{tiny_skia, usvg};

pub const APP_NAME: &str = "AnimeIAI";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// A marca certa para o tema (o fundo do quadrado acompanha o tema da interface).
pub fn logo_for(theme: ThemeKind) -> IconId {
    match theme {
        ThemeKind::Dark => IconId::Logo,
        ThemeKind::Light => IconId::LogoLight,
    }
}

// ------------------------------------------------------------------------------------------------------ wordmark

const WORDMARK_DARK: &str = include_str!("../assets/brand/logo-wordmark-dark.svg");
const WORDMARK_LIGHT: &str = include_str!("../assets/brand/logo-wordmark-light.svg");

/// SVG da marca com o nome ("AnimeIAI") para o tema da interface.
pub fn wordmark_svg(theme: ThemeKind) -> &'static str {
    match theme {
        ThemeKind::Dark => WORDMARK_DARK,
        ThemeKind::Light => WORDMARK_LIGHT,
    }
}

/// Rasteriza `svg` com `width` px de largura e a altura que o viewBox pede (RGBA premultiplicado): `(largura, altura, pixels)`.
pub fn rasterize_width(svg: &str, width: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let width = width.clamp(1, 4096);
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let scale = width as f32 / size.width();
    let height = ((size.height() * scale).round() as u32).max(1);
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("pixmap inválido")?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok((width, height, pixmap.take()))
}

/// Textura (em cache no contexto egui) do wordmark para `width_pts` pontos de largura; devolve o id e o tamanho em pontos.
pub fn wordmark_texture(ctx: &egui::Context, theme: ThemeKind, width_pts: f32) -> (egui::TextureId, egui::Vec2) {
    let px = ((width_pts * ctx.pixels_per_point()).round() as u32).max(8);
    let id = egui::Id::new(("brand-wordmark", theme as u8, px));
    let tex = ctx.data_mut(|d| d.get_temp::<egui::TextureHandle>(id));
    let tex = tex.unwrap_or_else(|| {
        let img = match rasterize_width(wordmark_svg(theme), px) {
            Ok((w, h, data)) => egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &data),
            Err(e) => {
                tracing::error!("wordmark: {e}");
                egui::ColorImage::filled([px as usize, (px / 4).max(1) as usize], egui::Color32::MAGENTA)
            }
        };
        let h = ctx.load_texture(format!("brand-wordmark-{px}"), img, egui::TextureOptions::LINEAR);
        ctx.data_mut(|d| d.insert_temp(id, h.clone()));
        h
    });
    let [w, h] = tex.size();
    let scale = width_pts / w as f32;
    (tex.id(), egui::vec2(w as f32 * scale, h as f32 * scale))
}

// ------------------------------------------------------------------------------------------------------ ícone do app

/// Variante do ícone do programa: o quadrado escuro (a do exe) ou o claro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconTheme {
    Dark,
    Light,
}

/// Desenhos na grade de pixels (16, 20, 24 e 32 px): (px, escuro, claro). Dali em diante vale a marca completa.
const HINTED: [(u32, &str, &str); 4] = [
    (16, include_str!("../assets/brand/hinted/logo-16.svg"), include_str!("../assets/brand/hinted/logo-16-light.svg")),
    (20, include_str!("../assets/brand/hinted/logo-20.svg"), include_str!("../assets/brand/hinted/logo-20-light.svg")),
    (24, include_str!("../assets/brand/hinted/logo-24.svg"), include_str!("../assets/brand/hinted/logo-24-light.svg")),
    (32, include_str!("../assets/brand/hinted/logo-32.svg"), include_str!("../assets/brand/hinted/logo-32-light.svg")),
];

/// SVG do ícone para `px` pixels: o desenho na grade de pixels mais próximo até 32 px, a marca completa acima disso.
pub fn icon_svg(theme: IconTheme, px: u32) -> &'static str {
    if px > 32 {
        return match theme {
            IconTheme::Dark => IconId::Logo.svg(),
            IconTheme::Light => IconId::LogoLight.svg(),
        };
    }
    let (_, dark, light) = HINTED.iter().min_by_key(|(n, _, _)| n.abs_diff(px)).copied().expect("HINTED não é vazio");
    match theme {
        IconTheme::Dark => dark,
        IconTheme::Light => light,
    }
}

/// Pixels RGBA (não premultiplicados) do ícone em `px` x `px`.
pub fn icon_rgba(theme: IconTheme, px: u32) -> Result<Vec<u8>, String> {
    let (n, mut data) = rasterize(icon_svg(theme, px), px)?;
    debug_assert_eq!(n, px);
    for p in data.chunks_exact_mut(4) {
        let a = p[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut p[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(data)
}

/// `IconData` do eframe para `px` x `px` (RGBA não premultiplicado, como o `IconData` exige).
pub fn window_icon_for(theme: IconTheme, px: u32) -> egui::IconData {
    match icon_rgba(theme, px) {
        Ok(rgba) => egui::IconData { rgba, width: px, height: px },
        Err(e) => {
            tracing::warn!("ícone da janela inválido: {e}");
            egui::IconData::default()
        }
    }
}

/// Ícone inicial da janela para `ViewportBuilder::with_icon`: tamanho do ícone grande do sistema, variante que contrasta
/// com a barra de tarefas.
pub fn window_icon() -> egui::IconData {
    let (_, big) = sys::icon_sizes();
    window_icon_for(sys::system_icon_theme(), big)
}

/// Variante do ícone para a barra de tarefas atual: quadrado escuro numa barra clara, quadrado claro numa escura.
pub fn system_icon_theme() -> IconTheme {
    sys::system_icon_theme()
}

/// Deixa o processo ciente de DPI por monitor antes de qualquer janela (o winit faria o mesmo na sua; sem isso o
/// Windows virtualiza os tamanhos de ícone e a tela de abertura sairia borrada em telas com escala).
pub fn enable_dpi_awareness() {
    sys::enable_dpi_awareness();
}

/// Define os ícones pequeno e grande da janela (títulos, Alt+Tab, barra de tarefas) a partir do `hwnd` do eframe.
/// Depois, `tick_window_icons` troca a variante quando o tema do Windows muda.
pub fn init_window_icons(window: &impl raw_window_handle::HasWindowHandle) {
    if let Ok(h) = window.window_handle() {
        if let raw_window_handle::RawWindowHandle::Win32(w) = h.as_raw() {
            sys::init_window_icons(w.hwnd.get());
        }
    }
}

/// Chamado a cada quadro: no máximo a cada 2 s confere o tema do Windows e refaz os ícones se mudou.
pub fn tick_window_icons() {
    sys::tick_window_icons();
}

#[cfg(windows)]
mod sys {
    use super::{icon_rgba, IconTheme};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIcon, DestroyIcon, SendMessageW, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, WM_SETICON};

    pub fn enable_dpi_awareness() {
        // falha (já definido, p. ex. por manifesto) é inofensiva: vale o que já estava
        unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
    }

    /// (pequeno, grande) em pixels para o DPI do sistema: 16/32 a 100%, 20/40 a 125%, 24/48 a 150%.
    pub fn icon_sizes() -> (u32, u32) {
        let dpi = unsafe { GetDpiForSystem() };
        sizes_for_dpi(if dpi == 0 { 96 } else { dpi })
    }

    fn sizes_for_dpi(dpi: u32) -> (u32, u32) {
        let (s, b) = unsafe { (GetSystemMetricsForDpi(SM_CXSMICON, dpi), GetSystemMetricsForDpi(SM_CXICON, dpi)) };
        (if s > 0 { s as u32 } else { 16 }, if b > 0 { b as u32 } else { 32 })
    }

    fn reg_dword(name: &str) -> Option<u32> {
        let key: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0".encode_utf16().collect();
        let value: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let mut data: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        let rc = unsafe { RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut data as *mut u32 as *mut _, &mut size) };
        (rc == 0).then_some(data)
    }

    /// Variante para a barra de tarefas (`SystemUsesLightTheme`; sem a chave, o tema dos aplicativos; sem nenhuma, barra
    /// escura): barra clara pede o quadrado escuro; barra escura, o claro.
    pub fn system_icon_theme() -> IconTheme {
        match reg_dword("SystemUsesLightTheme").or_else(|| reg_dword("AppsUseLightTheme")) {
            Some(1) => IconTheme::Dark,
            _ => IconTheme::Light,
        }
    }

    struct State {
        hwnd: isize,
        theme: IconTheme,
        checked: Instant,
        /// HICONs criados aqui (pequeno, grande); o que o winit pôs antes não é nosso e não se destrói.
        mine: [isize; 2],
    }
    static STATE: Mutex<Option<State>> = Mutex::new(None);

    fn make_icon(theme: IconTheme, px: u32) -> Option<isize> {
        let rgba = match icon_rgba(theme, px) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("ícone {px}px: {e}");
                return None;
            }
        };
        // BGRA para o ícone e máscara AND zerada (o alfa manda); mesma construção do winit
        let mut bgra = rgba;
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        let mask_row = (px as usize).div_ceil(32) * 4;
        let and_mask = vec![0u8; mask_row * px as usize];
        let h = unsafe { CreateIcon(std::ptr::null_mut(), px as i32, px as i32, 1, 32, and_mask.as_ptr(), bgra.as_ptr()) };
        (!h.is_null()).then_some(h as isize)
    }

    fn apply(st: &mut State) {
        let hwnd = st.hwnd as HWND;
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        let (small, big) = sizes_for_dpi(if dpi == 0 { 96 } else { dpi });
        for (slot, (kind, px)) in [(ICON_SMALL, small), (ICON_BIG, big)].into_iter().enumerate() {
            let Some(icon) = make_icon(st.theme, px) else { continue };
            unsafe { SendMessageW(hwnd, WM_SETICON, kind as usize, icon) };
            if st.mine[slot] != 0 {
                unsafe { DestroyIcon(st.mine[slot] as _) };
            }
            st.mine[slot] = icon;
        }
    }

    pub fn init_window_icons(hwnd: isize) {
        let mut st = State { hwnd, theme: system_icon_theme(), checked: Instant::now(), mine: [0, 0] };
        apply(&mut st);
        *STATE.lock().unwrap() = Some(st);
    }

    pub fn tick_window_icons() {
        let Ok(mut g) = STATE.try_lock() else { return };
        let Some(st) = g.as_mut() else { return };
        if st.checked.elapsed() < Duration::from_secs(2) {
            return;
        }
        st.checked = Instant::now();
        let now = system_icon_theme();
        if now != st.theme {
            st.theme = now;
            apply(st);
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::IconTheme;
    pub fn enable_dpi_awareness() {}
    pub fn icon_sizes() -> (u32, u32) {
        (16, 32)
    }
    pub fn system_icon_theme() -> IconTheme {
        IconTheme::Light
    }
    pub fn init_window_icons(_hwnd: isize) {}
    pub fn tick_window_icons() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_icon_is_a_square_with_content_in_every_system_size() {
        for px in [16, 20, 24, 32, 40, 48, 64, 256] {
            for theme in [IconTheme::Dark, IconTheme::Light] {
                let ic = window_icon_for(theme, px);
                assert_eq!((ic.width, ic.height), (px, px));
                assert_eq!(ic.rgba.len(), (px * px * 4) as usize);
                // o quadrado da marca cobre a maior parte do ícone
                let solid = ic.rgba.chunks_exact(4).filter(|p| p[3] == 255).count();
                assert!(solid > (px * px) as usize / 2, "{theme:?} {px}: só {solid} px opacos");
            }
        }
    }

    #[test]
    fn small_sizes_use_the_pixel_grid_drawings_and_big_ones_the_full_mark() {
        assert_eq!(icon_svg(IconTheme::Dark, 16), HINTED[0].1);
        assert_eq!(icon_svg(IconTheme::Light, 24), HINTED[2].2);
        // entre dois desenhos vale o mais próximo (28 está a 4 de 32 e a 4 de 24: o primeiro de menor distância)
        assert!(HINTED.iter().any(|(_, d, _)| *d == icon_svg(IconTheme::Dark, 28)));
        assert_eq!(icon_svg(IconTheme::Dark, 48), IconId::Logo.svg());
        assert_eq!(icon_svg(IconTheme::Light, 256), IconId::LogoLight.svg());
    }

    #[test]
    fn dark_and_light_icons_differ() {
        let d = icon_rgba(IconTheme::Dark, 48).unwrap();
        let l = icon_rgba(IconTheme::Light, 48).unwrap();
        assert_ne!(d, l);
        // centro do quadrado: escuro num, claro no outro
        let c = (24 * 48 + 4) * 4;
        assert!(d[c] < 80 && l[c] > 200, "{} {}", d[c], l[c]);
    }

    #[test]
    fn wordmark_keeps_its_aspect_and_draws_both_themes() {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            let (w, h, data) = rasterize_width(wordmark_svg(theme), 560).unwrap();
            assert_eq!(w, 560);
            assert!((115..=135).contains(&h), "altura {h}"); // 560 x ~128: o lockup tem proporção 4,4:1
            assert_eq!(data.len(), (w * h * 4) as usize);
            assert!(data.chunks_exact(4).filter(|p| p[3] > 8).count() > 5000);
        }
    }

    /// PNGs da marca para entrega/apresentação: `AIAI_BRAND_OUT=<pasta> cargo test -p aiai-studio brand_pngs -- --ignored`.
    /// (Os PNGs versionados em `assets/brand/png/` saem de `tools/brand/make_icons.py`; este teste confere o caminho do app.)
    #[test]
    #[ignore]
    fn brand_pngs() {
        let Some(out) = std::env::var_os("AIAI_BRAND_OUT") else { return };
        let out = std::path::PathBuf::from(out);
        std::fs::create_dir_all(&out).unwrap();
        for px in [16, 20, 24, 32, 40, 48, 64, 128, 256] {
            for (tag, theme) in [("dark", IconTheme::Dark), ("light", IconTheme::Light)] {
                let rgba = icon_rgba(theme, px).unwrap();
                image::save_buffer(out.join(format!("icon-{tag}-{px}.png")), &rgba, px, px, image::ColorType::Rgba8).unwrap();
            }
        }
        for (tag, theme) in [("dark", ThemeKind::Dark), ("light", ThemeKind::Light)] {
            let (w, h, mut data) = rasterize_width(wordmark_svg(theme), 1024).unwrap();
            for p in data.chunks_exact_mut(4) {
                let a = p[3] as u32;
                if a > 0 && a < 255 {
                    for c in &mut p[..3] {
                        *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
                    }
                }
            }
            image::save_buffer(out.join(format!("wordmark-{tag}.png")), &data, w, h, image::ColorType::Rgba8).unwrap();
        }
    }
}
