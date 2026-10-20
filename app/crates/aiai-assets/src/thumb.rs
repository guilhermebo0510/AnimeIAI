//! Miniaturas e prévias na CPU (rodam numa thread de fundo no app): imagem reduzida, pincel (a miniatura do arquivo) e
//! modelo 3D pelo rasterizador de reserva do `aiai-3d-model` (3/4 com giro e câmera que enquadra o modelo). Paleta, pose e
//! material toon não têm imagem: a tela desenha amostras e ícones.

use crate::index::Entry;
use crate::kind::{IMAGE_EXTS, Kind, ext_of};
use aiai_3d_model::{Model, camera::framing_camera, raster};
use aiai_core::BlobHash;
use aiai_core::model3d::Model3dState;
use aiai_paint::brush_pack::{self, Package};
use std::path::Path;
use std::sync::Arc;

/// Imagem RGBA 8 bits **pré-multiplicada** (o formato que o compositor e o `egui` esperam das camadas).
#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Teto de triângulos para o rasterizador CPU fazer miniatura em tempo razoável (acima disso a prévia mostra só o aviso).
pub const CPU_PREVIEW_MAX_TRIS: usize = 400_000;

fn premultiply(img: &image::RgbaImage) -> Vec<u8> {
    let mut v = Vec::with_capacity(img.as_raw().len());
    for p in img.pixels() {
        let a = p[3] as u32;
        v.extend([((p[0] as u32 * a + 127) / 255) as u8, ((p[1] as u32 * a + 127) / 255) as u8, ((p[2] as u32 * a + 127) / 255) as u8, p[3]]);
    }
    v
}

/// Imagem do disco reduzida para caber em `size` x `size` (sem ampliar).
pub fn image_thumb(path: &Path, size: u32) -> Result<Thumb, String> {
    let img = image::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let img = if img.width() > size || img.height() > size { img.thumbnail(size, size) } else { img };
    let rgba = img.to_rgba8();
    Ok(Thumb { w: rgba.width(), h: rgba.height(), rgba: premultiply(&rgba) })
}

/// Estado de câmera de um modelo girando: enquadra a caixa, `yaw_deg` em volta do alvo, toon com contorno.
pub fn model_state(model: &Model, hash: &BlobHash, yaw_deg: f32, aspect: f32) -> Model3dState {
    let mut camera = framing_camera(model.info.bbox, aspect);
    camera.yaw_deg = yaw_deg;
    camera.pitch_deg = 14.0;
    // montado por JSON: campos novos (opcionais) do estado do núcleo não quebram esta crate
    serde_json::from_value(serde_json::json!({
        "model_hash": hash, "transform": aiai_core::model3d::Transform3d::default(), "camera": camera,
        "mode": "toon", "outline_px": 1.5, "outline_color": [0.11, 0.12, 0.15], "color": null,
    }))
    .expect("estado 3D da miniatura")
}

/// Lê o modelo de um arquivo da biblioteca (`.glb` de terceiros, `.gltf`, `.obj`) e dá o hash dele.
pub fn load_model(path: &Path) -> Result<(Arc<Model>, BlobHash), String> {
    let model = aiai_3d_model::import_path(path).map_err(|e| e.0)?;
    let hash = BlobHash::of(&aiai_3d_model::glb::write_glb(&model));
    Ok((Arc::new(model), hash))
}

/// Quadro do modelo no rasterizador CPU (pré-multiplicado).
pub fn model_frame_cpu(model: &Model, hash: &BlobHash, yaw_deg: f32, w: u32, h: u32) -> Thumb {
    let st = model_state(model, hash, yaw_deg, w as f32 / h.max(1) as f32);
    let px = raster::render_cpu(model, &st, w, h);
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    Thumb { w, h, rgba: px.iter().flat_map(|p| [b(p[0]), b(p[1]), b(p[2]), b(p[3])]).collect() }
}

/// Miniatura de um recurso: `None` se o tipo não tem imagem (paleta, pose, material toon).
pub fn thumbnail(e: &Entry, size: u32) -> Result<Option<Thumb>, String> {
    match e.kind {
        Kind::Texture | Kind::Material if IMAGE_EXTS.contains(&ext_of(&e.path).as_str()) => image_thumb(&e.path, size).map(Some),
        Kind::Model3d => {
            let (model, hash) = load_model(&e.path)?;
            if model.info.triangles > CPU_PREVIEW_MAX_TRIS {
                return Err(format!("modelo com {} triângulos: miniatura pulada", model.info.triangles));
            }
            Ok(Some(model_frame_cpu(&model, &hash, 30.0, size, size)))
        }
        Kind::Brush => {
            let bytes = std::fs::read(&e.path).map_err(|er| format!("{}: {er}", e.path.display()))?;
            let Package::Brush(b) = brush_pack::read(&bytes)? else { return Ok(None) };
            let png = match &b.thumbnail {
                Some(t) => t.clone(),
                None => brush_pack::render_thumbnail(&b.preset)?,
            };
            let img = image::load_from_memory(&png).map_err(|er| er.to_string())?;
            let img = if img.width() > size || img.height() > size { img.thumbnail(size, size) } else { img };
            let rgba = img.to_rgba8();
            Ok(Some(Thumb { w: rgba.width(), h: rgba.height(), rgba: premultiply(&rgba) }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiply_scales_color_by_alpha() {
        let mut img = image::RgbaImage::new(1, 1);
        img.put_pixel(0, 0, image::Rgba([200, 100, 50, 128]));
        assert_eq!(premultiply(&img), vec![100, 50, 25, 128]);
    }

    #[test]
    fn image_thumbnail_fits_the_box_and_a_model_turns() {
        let d = std::env::temp_dir().join(format!("aiai-assets-thumb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("a.png");
        image::RgbaImage::from_pixel(400, 100, image::Rgba([10, 200, 30, 255])).save(&p).unwrap();
        let t = image_thumb(&p, 128).unwrap();
        assert_eq!((t.w, t.h), (128, 32));
        assert_eq!(t.rgba.len(), 128 * 32 * 4);
        let obj = "v 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0 0 1\nv 1 0 1\nv 1 1 1\nv 0 1 1\nf 1 2 3 4\nf 5 8 7 6\nf 1 5 6 2\nf 2 6 7 3\nf 3 7 8 4\nf 5 1 4 8\n";
        let m = aiai_3d_model::import_bytes(obj.as_bytes(), "obj", "c.obj", &|_| None).unwrap();
        let h = BlobHash::of(b"c");
        let a = model_frame_cpu(&m, &h, 0.0, 64, 64);
        let b = model_frame_cpu(&m, &h, 45.0, 64, 64);
        assert!(a.rgba.chunks(4).any(|p| p[3] > 0), "o modelo aparece");
        assert_ne!(a.rgba, b.rgba, "girar muda o quadro");
        let _ = std::fs::remove_dir_all(&d);
    }
}
