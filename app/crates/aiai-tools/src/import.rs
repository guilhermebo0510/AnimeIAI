//! Importar imagem do disco para um cel: referência (imagem guardada no cel) ou raster (vira tiles editáveis).
//! Formatos: os que a crate `image` do workspace lê (PNG, JPEG, WebP, BMP, TIFF, GIF — primeiro quadro).

use crate::context::*;
use crate::paint::{cel_raster, tiles_for_region};
use aiai_core::*;
use aiai_paint::Image;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    /// Cabe inteira no canvas, centralizada (padrão).
    #[default]
    Contain,
    /// Cobre o canvas inteiro, centralizada (corta as sobras).
    Cover,
    /// Tamanho original, no canto de cima à esquerda.
    Actual,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImportImageRequest {
    pub cut: Id,
    /// Camada de referência (a imagem fica no cel) ou raster (a imagem vira tiles e pode ser pintada por cima).
    pub layer: Id,
    #[serde(default)]
    pub frame: u32,
    /// Caminho absoluto do arquivo de imagem.
    pub path: String,
    #[serde(default)]
    pub fit: ImageFit,
}

/// Imagem do arquivo ajustada ao canvas `w`×`h` (pixels transparentes em volta).
pub fn fitted_image(path: &str, w: u32, h: u32, fit: ImageFit) -> ToolResult<Image> {
    let src = ::image::open(path).map_err(|e| ToolError::Invalid(format!("não consegui abrir {path}: {e}")))?.to_rgba8();
    let (iw, ih) = (src.width().max(1), src.height().max(1));
    let s = match fit {
        ImageFit::Contain => (w as f32 / iw as f32).min(h as f32 / ih as f32),
        ImageFit::Cover => (w as f32 / iw as f32).max(h as f32 / ih as f32),
        ImageFit::Actual => 1.0,
    };
    let (nw, nh) = (((iw as f32 * s).round() as u32).max(1), ((ih as f32 * s).round() as u32).max(1));
    let scaled = if (nw, nh) == (iw, ih) { src } else { ::image::imageops::resize(&src, nw, nh, ::image::imageops::FilterType::Lanczos3) };
    let (ox, oy) = match fit {
        ImageFit::Actual => (0i64, 0i64),
        _ => ((w as i64 - nw as i64) / 2, (h as i64 - nh as i64) / 2),
    };
    let mut canvas = ::image::RgbaImage::new(w, h);
    ::image::imageops::overlay(&mut canvas, &scaled, ox, oy);
    Ok(Image::from_rgba8(w, h, canvas.as_raw()))
}

pub fn import_image_command(p: &Project, blobs: &mut BlobStore, req: &ImportImageRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    let img = fitted_image(&req.path, w, h, req.fit)?;
    match layer.kind {
        LayerType::Raster => {
            // por cima do que já está pintado no cel
            let mut base = cel_raster(blobs, cel, w, h)?;
            base.draw_over(&img, 0, 0, 1.0);
            Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &base, [0, 0, w as i32, h as i32]) })
        }
        LayerType::Reference => {
            let png = img.encode_png().map_err(ToolError::Render)?;
            let hash = blobs.put(png);
            Ok(Command::SetProps { id: node.id.clone(), name: None, data: json!({ "image": hash }) })
        }
        other => Err(ToolError::Invalid(format!("imagem entra em camada de referência ou raster, não {other:?}"))),
    }
}
