//! Ícones SVG originais -> textura egui.
//!
//! Os SVG (grade de 24 px, traço de 1,75 px, `currentColor`) vivem em `assets/icons/`. Cada ícone é rasterizado com
//! resvg em branco no tamanho exato `pontos x pixels_per_point` (nítido em qualquer DPI/zoom), guardado em cache e
//! pintado com `tint` = a cor do tema (é o "currentColor recolorido por tema"). Nada de emoji nem fonte de ícones.

use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions};
use resvg::{tiny_skia, usvg};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconId {
    WsRoteiro,
    WsPersonagens,
    WsStoryboard,
    WsLayout,
    WsAnimacao,
    WsPintura,
    WsCenario,
    WsComposicao,
    WsBiblioteca,
    WsModelagem,
    WsAudio,
    WsEdicao,
    WsEntrega,
    WsProducao,
    ToolMove,
    ToolBrush,
    ToolPen,
    ToolPoints,
    ToolEraser,
    ToolFill,
    ToolLasso,
    ToolEyedropper,
    ToolHand,
    ToolZoom,
    ToolText,
    ZoomOut,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Plus,
    Trash,
    ArrowUp,
    ArrowDown,
    Copy,
    LayerAdd,
    LayerVector,
    LayerRaster,
    LayerFill,
    LayerGroup,
    LayerGuide,
    LayerReference,
    LayerEffect,
    LayerParticles,
    LayerText,
    LayerAdjustment,
    Play,
    Pause,
    Loop,
    StepBack,
    StepForward,
    Fit,
    OneToOne,
    RotateView,
    Flip,
    CameraFrame,
    Grid,
    ChevronRight,
    ChevronDown,
    NodeEpisode,
    NodeScene,
    NodeCut,
    NodeCharacter,
    NodeDocument,
    /// Marca do app (colorida, não é recolorida).
    Logo,
    /// A marca para o tema claro.
    LogoLight,
    /// A marca simplificada para 16 a 32 px (barra de menus, ícone pequeno).
    LogoSmall,
}

impl IconId {
    pub const ALL: [IconId; 67] = [
        IconId::WsRoteiro,
        IconId::WsPersonagens,
        IconId::WsStoryboard,
        IconId::WsLayout,
        IconId::WsAnimacao,
        IconId::WsPintura,
        IconId::WsCenario,
        IconId::WsComposicao,
        IconId::WsBiblioteca,
        IconId::WsModelagem,
        IconId::WsAudio,
        IconId::WsEdicao,
        IconId::WsEntrega,
        IconId::WsProducao,
        IconId::ToolMove,
        IconId::ToolBrush,
        IconId::ToolPen,
        IconId::ToolPoints,
        IconId::ToolEraser,
        IconId::ToolFill,
        IconId::ToolLasso,
        IconId::ToolEyedropper,
        IconId::ToolHand,
        IconId::ToolZoom,
        IconId::ToolText,
        IconId::ZoomOut,
        IconId::Eye,
        IconId::EyeOff,
        IconId::Lock,
        IconId::Unlock,
        IconId::Plus,
        IconId::Trash,
        IconId::ArrowUp,
        IconId::ArrowDown,
        IconId::Copy,
        IconId::LayerAdd,
        IconId::LayerVector,
        IconId::LayerRaster,
        IconId::LayerFill,
        IconId::LayerGroup,
        IconId::LayerGuide,
        IconId::LayerReference,
        IconId::LayerEffect,
        IconId::LayerParticles,
        IconId::LayerText,
        IconId::LayerAdjustment,
        IconId::Play,
        IconId::Pause,
        IconId::Loop,
        IconId::StepBack,
        IconId::StepForward,
        IconId::Fit,
        IconId::OneToOne,
        IconId::RotateView,
        IconId::Flip,
        IconId::CameraFrame,
        IconId::Grid,
        IconId::ChevronRight,
        IconId::ChevronDown,
        IconId::NodeEpisode,
        IconId::NodeScene,
        IconId::NodeCut,
        IconId::NodeCharacter,
        IconId::NodeDocument,
        IconId::Logo,
        IconId::LogoLight,
        IconId::LogoSmall,
    ];

    /// Marca colorida (não segue a grade de 24 px nem a cor do texto; pintada sem tingimento).
    pub fn is_brand(self) -> bool {
        matches!(self, IconId::Logo | IconId::LogoLight | IconId::LogoSmall)
    }

    pub fn svg(self) -> &'static str {
        match self {
            IconId::WsRoteiro => include_str!("../assets/icons/ws-roteiro.svg"),
            IconId::WsPersonagens => include_str!("../assets/icons/ws-personagens.svg"),
            IconId::WsStoryboard => include_str!("../assets/icons/ws-storyboard.svg"),
            IconId::WsLayout => include_str!("../assets/icons/ws-layout.svg"),
            IconId::WsAnimacao => include_str!("../assets/icons/ws-animacao.svg"),
            IconId::WsPintura => include_str!("../assets/icons/ws-pintura.svg"),
            IconId::WsCenario => include_str!("../assets/icons/ws-cenario.svg"),
            IconId::WsComposicao => include_str!("../assets/icons/ws-composicao.svg"),
            IconId::WsBiblioteca => include_str!("../assets/icons/ws-biblioteca.svg"),
            IconId::WsModelagem => include_str!("../assets/icons/ws-modelagem.svg"),
            IconId::WsAudio => include_str!("../assets/icons/ws-audio.svg"),
            IconId::WsEdicao => include_str!("../assets/icons/ws-edicao.svg"),
            IconId::WsEntrega => include_str!("../assets/icons/ws-entrega.svg"),
            IconId::WsProducao => include_str!("../assets/icons/ws-producao.svg"),
            IconId::ToolMove => include_str!("../assets/icons/tool-move.svg"),
            IconId::ToolBrush => include_str!("../assets/icons/tool-brush.svg"),
            IconId::ToolPen => include_str!("../assets/icons/tool-pen.svg"),
            IconId::ToolPoints => include_str!("../assets/icons/tool-points.svg"),
            IconId::ToolEraser => include_str!("../assets/icons/tool-eraser.svg"),
            IconId::ToolFill => include_str!("../assets/icons/tool-fill.svg"),
            IconId::ToolLasso => include_str!("../assets/icons/tool-lasso.svg"),
            IconId::ToolEyedropper => include_str!("../assets/icons/tool-eyedropper.svg"),
            IconId::ToolHand => include_str!("../assets/icons/tool-hand.svg"),
            IconId::ToolZoom => include_str!("../assets/icons/tool-zoom.svg"),
            IconId::ToolText => include_str!("../assets/icons/tool-text.svg"),
            IconId::ZoomOut => include_str!("../assets/icons/zoom-out.svg"),
            IconId::Eye => include_str!("../assets/icons/eye.svg"),
            IconId::EyeOff => include_str!("../assets/icons/eye-off.svg"),
            IconId::Lock => include_str!("../assets/icons/lock.svg"),
            IconId::Unlock => include_str!("../assets/icons/unlock.svg"),
            IconId::Plus => include_str!("../assets/icons/plus.svg"),
            IconId::Trash => include_str!("../assets/icons/trash.svg"),
            IconId::ArrowUp => include_str!("../assets/icons/arrow-up.svg"),
            IconId::ArrowDown => include_str!("../assets/icons/arrow-down.svg"),
            IconId::Copy => include_str!("../assets/icons/copy.svg"),
            IconId::LayerAdd => include_str!("../assets/icons/layer-add.svg"),
            IconId::LayerVector => include_str!("../assets/icons/layer-vector.svg"),
            IconId::LayerRaster => include_str!("../assets/icons/layer-raster.svg"),
            IconId::LayerFill => include_str!("../assets/icons/layer-fill.svg"),
            IconId::LayerGroup => include_str!("../assets/icons/layer-group.svg"),
            IconId::LayerGuide => include_str!("../assets/icons/layer-guide.svg"),
            IconId::LayerReference => include_str!("../assets/icons/layer-reference.svg"),
            IconId::LayerEffect => include_str!("../assets/icons/layer-effect.svg"),
            IconId::LayerParticles => include_str!("../assets/icons/layer-particles.svg"),
            IconId::LayerText => include_str!("../assets/icons/layer-text.svg"),
            IconId::LayerAdjustment => include_str!("../assets/icons/layer-adjustment.svg"),
            IconId::Play => include_str!("../assets/icons/play.svg"),
            IconId::Pause => include_str!("../assets/icons/pause.svg"),
            IconId::Loop => include_str!("../assets/icons/loop.svg"),
            IconId::StepBack => include_str!("../assets/icons/step-back.svg"),
            IconId::StepForward => include_str!("../assets/icons/step-forward.svg"),
            IconId::Fit => include_str!("../assets/icons/fit.svg"),
            IconId::OneToOne => include_str!("../assets/icons/one-to-one.svg"),
            IconId::RotateView => include_str!("../assets/icons/rotate-view.svg"),
            IconId::Flip => include_str!("../assets/icons/flip.svg"),
            IconId::CameraFrame => include_str!("../assets/icons/camera-frame.svg"),
            IconId::Grid => include_str!("../assets/icons/grid.svg"),
            IconId::ChevronRight => include_str!("../assets/icons/chevron-right.svg"),
            IconId::ChevronDown => include_str!("../assets/icons/chevron-down.svg"),
            IconId::NodeEpisode => include_str!("../assets/icons/node-episode.svg"),
            IconId::NodeScene => include_str!("../assets/icons/node-scene.svg"),
            IconId::NodeCut => include_str!("../assets/icons/node-cut.svg"),
            IconId::NodeCharacter => include_str!("../assets/icons/node-character.svg"),
            IconId::NodeDocument => include_str!("../assets/icons/node-document.svg"),
            IconId::Logo => include_str!("../assets/brand/logo.svg"),
            IconId::LogoLight => include_str!("../assets/brand/logo-light.svg"),
            IconId::LogoSmall => include_str!("../assets/brand/logo-small.svg"),
        }
    }
}

/// Rasteriza `svg` em `px` x `px` (RGBA premultiplicado). `currentColor` vira branco: o recolorido é feito na pintura.
pub fn rasterize(svg: &str, px: u32) -> Result<(u32, Vec<u8>), String> {
    let px = px.clamp(1, 2048);
    let svg = svg.replace("currentColor", "#ffffff");
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let mut pixmap = tiny_skia::Pixmap::new(px, px).ok_or("pixmap inválido")?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    resvg::render(&tree, tiny_skia::Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    Ok((px, pixmap.take()))
}

/// Cache de texturas de ícones por (ícone, tamanho em pixels).
#[derive(Default)]
pub struct Icons {
    cache: HashMap<(IconId, u32), TextureHandle>,
}

impl Icons {
    /// Textura do ícone para `size_pts` pontos de lado no `pixels_per_point` atual.
    pub fn texture(&mut self, ctx: &egui::Context, id: IconId, size_pts: f32) -> egui::TextureId {
        let px = ((size_pts * ctx.pixels_per_point()).round() as u32).max(1);
        let tex = self.cache.entry((id, px)).or_insert_with(|| {
            let img = match rasterize(id.svg(), px) {
                Ok((px, data)) => ColorImage::from_rgba_premultiplied([px as usize, px as usize], &data),
                Err(e) => {
                    tracing::error!("ícone {id:?}: {e}");
                    ColorImage::filled([px as usize, px as usize], Color32::MAGENTA)
                }
            };
            ctx.load_texture(format!("icon-{id:?}-{px}"), img, TextureOptions::LINEAR)
        });
        tex.id()
    }

    /// Pinta o ícone centrado em `rect` (lado = menor dimensão) com a cor `tint`.
    pub fn paint(&mut self, painter: &egui::Painter, id: IconId, rect: Rect, tint: Color32) {
        let side = rect.width().min(rect.height());
        let r = Rect::from_center_size(rect.center(), egui::vec2(side, side));
        let tex = self.texture(painter.ctx(), id, side);
        painter.image(tex, r, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), tint);
    }

    pub fn cached(&self) -> usize {
        self.cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_svg_parses_and_draws_something() {
        for id in IconId::ALL {
            let (px, data) = rasterize(id.svg(), 48).unwrap_or_else(|e| panic!("{id:?}: {e}"));
            assert_eq!(data.len(), (px * px * 4) as usize);
            let opaque = data.chunks_exact(4).filter(|p| p[3] > 8).count();
            assert!(opaque > 40, "{id:?} ficou vazio ({opaque} px)");
            assert!(opaque < (48 * 48) as usize * 9 / 10, "{id:?} ficou um bloco cheio");
        }
    }

    #[test]
    fn monochrome_icons_follow_the_24px_grid_with_current_color() {
        for id in IconId::ALL {
            if id.is_brand() {
                continue;
            }
            let s = id.svg();
            assert!(s.contains("viewBox=\"0 0 24 24\""), "{id:?}");
            assert!(s.contains("stroke=\"currentColor\"") && s.contains("stroke-width=\"1.75\""), "{id:?}");
            assert!(!s.contains("#"), "{id:?}: cor fixa em ícone monocromático");
        }
    }

    #[test]
    fn raster_size_follows_the_request() {
        let (a, _) = rasterize(IconId::ToolMove.svg(), 20).unwrap();
        let (b, d) = rasterize(IconId::ToolMove.svg(), 60).unwrap();
        assert_eq!((a, b), (20, 60));
        assert_eq!(d.len(), 60 * 60 * 4);
    }

    /// Gera uma folha de contato para inspeção visual: `AIAI_ICON_SHEET=<png> cargo test -p aiai-studio icon_sheet -- --ignored`.
    #[test]
    #[ignore]
    fn icon_sheet() {
        let Some(out) = std::env::var_os("AIAI_ICON_SHEET") else { return };
        let cell = 96u32;
        let cols = 8u32;
        let rows = (IconId::ALL.len() as u32).div_ceil(cols);
        let (w, h) = (cols * cell, rows * cell);
        let mut img = image::RgbaImage::from_pixel(w, h, image::Rgba([0x22, 0x23, 0x27, 255]));
        for (i, id) in IconId::ALL.iter().enumerate() {
            let (px, data) = rasterize(id.svg(), 72).unwrap();
            let (ox, oy) = ((i as u32 % cols) * cell + 12, (i as u32 / cols) * cell + 12);
            for y in 0..px {
                for x in 0..px {
                    let p = &data[((y * px + x) * 4) as usize..][..4];
                    let a = p[3] as f32 / 255.0;
                    let dst = img.get_pixel_mut(ox + x, oy + y);
                    for c in 0..3 {
                        let src = if id.is_brand() { p[c] as f32 } else { p[c] as f32 * 0.87 };
                        dst.0[c] = (src + dst.0[c] as f32 * (1.0 - a)).min(255.0) as u8;
                    }
                }
            }
        }
        img.save(out).unwrap();
    }
}
