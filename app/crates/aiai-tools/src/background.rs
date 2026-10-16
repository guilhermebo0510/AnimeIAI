//! Cenário: céu, preenchimento por textura (plano ou em perspectiva por homografia) numa região.

use crate::context::*;
use crate::paint::{cel_raster, tiles_for_region};
use aiai_core::*;
use aiai_paint::blend::blend_pixel;
use aiai_paint::select::{SelectShape, Selection};
use aiai_paint::texture::{SkySpec, TextureParams};
use aiai_paint::warp::Homography;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkyRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub sky: SkySpec,
}

fn raster_target<'a>(p: &'a Project, cut: &Id, layer: &Id, frame: u32) -> ToolResult<(&'a Node, &'a CelData, u32, u32)> {
    let c = cut_data(p, cut)?;
    let l = layer_data(p, layer)?;
    if l.kind != LayerType::Raster {
        return Err(ToolError::Invalid("céu/textura pintam em camada raster".into()));
    }
    let node = exposed_cel(p, c, layer, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, c);
    Ok((node, cel, w, h))
}

pub fn sky_command(p: &Project, blobs: &mut BlobStore, req: &SkyRequest) -> ToolResult<Command> {
    let (node, _, w, h) = raster_target(p, &req.cut, &req.layer, req.frame)?;
    let img = req.sky.render(w, h);
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]) })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextureMapping {
    /// Textura plana: escala (multiplica o tamanho do grão), rotação (graus), deslocamento (px).
    Flat {
        #[serde(default = "one")]
        scale: f32,
        #[serde(default)]
        rotation: f32,
        #[serde(default)]
        offset: [f32; 2],
    },
    /// Textura mapeada num quadrilátero em perspectiva (cantos: sup-esq, sup-dir, inf-dir, inf-esq). `size` é o tamanho
    /// "real" do plano em px de textura (ex.: parede de 600×300 px de tijolo) — o padrão diminui com a distância.
    Perspective { quad: [[f32; 2]; 4], size: [f32; 2] },
}

fn one() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PatternFillRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub texture: TextureParams,
    pub mapping: TextureMapping,
    /// Região a preencher; omitida = o quadrilátero (perspectiva) ou o canvas inteiro (plano).
    #[serde(default)]
    pub region: Option<SelectShape>,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default = "one")]
    pub opacity: f32,
}

pub fn pattern_fill_command(p: &Project, blobs: &mut BlobStore, req: &PatternFillRequest) -> ToolResult<Command> {
    req.texture.validate().map_err(ToolError::Invalid)?;
    let (node, cel, w, h) = raster_target(p, &req.cut, &req.layer, req.frame)?;
    let mut img = cel_raster(blobs, cel, w, h)?;
    let region = match (&req.region, &req.mapping) {
        (Some(r), _) => Some(r.clone()),
        (None, TextureMapping::Perspective { quad, .. }) => Some(SelectShape::Lasso { points: quad.to_vec() }),
        (None, TextureMapping::Flat { .. }) => None,
    };
    let sel = match &region {
        Some(r) => {
            let reference = if matches!(r, SelectShape::Wand { .. } | SelectShape::ColorRange { .. }) { Some(img.clone()) } else { None };
            Selection::from_shape(r, w, h, reference.as_ref()).map_err(ToolError::Invalid)?
        }
        None => Selection::all(w, h),
    };
    let Some(bounds) = sel.bounds() else { return Err(ToolError::Invalid("região vazia".into())) };
    // mapeamento destino (px do canvas) → coordenada de textura (px de textura)
    enum Map {
        Flat { inv_s: f32, cos: f32, sin: f32, off: [f32; 2] },
        Persp { inv: Homography, size: [f32; 2] },
    }
    let map = match &req.mapping {
        TextureMapping::Flat { scale, rotation, offset } => {
            let (s, c) = (-rotation).to_radians().sin_cos();
            Map::Flat { inv_s: 1.0 / scale.max(0.01), cos: c, sin: s, off: *offset }
        }
        TextureMapping::Perspective { quad, size } => {
            let unit = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            let hm = Homography::from_quads(unit, *quad).ok_or_else(|| ToolError::Invalid("quadrilátero degenerado".into()))?;
            Map::Persp { inv: hm.inverse().ok_or_else(|| ToolError::Invalid("quadrilátero degenerado".into()))?, size: *size }
        }
    };
    let tex_at = |x: f32, y: f32| -> [f32; 4] {
        match &map {
            Map::Flat { inv_s, cos, sin, off } => {
                let (dx, dy) = (x - off[0], y - off[1]);
                req.texture.color_at((dx * cos - dy * sin) * inv_s, (dx * sin + dy * cos) * inv_s)
            }
            Map::Persp { inv, size } => {
                let (u, v) = inv.apply(x, y);
                req.texture.color_at(u * size[0], v * size[1])
            }
        }
    };
    let ss = [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)];
    for y in bounds[1]..bounds[3] {
        for x in bounds[0]..bounds[2] {
            let i = (y as u32 * w + x as u32) as usize;
            let cov = sel.cov[i];
            if cov <= 0.0 {
                continue;
            }
            // supersampling 2×2 contra serrilhado do padrão ao longe
            let mut acc = [0.0f32; 4];
            for (ox, oy) in ss {
                let c = tex_at(x as f32 + ox, y as f32 + oy);
                acc[0] += c[0] * c[3];
                acc[1] += c[1] * c[3];
                acc[2] += c[2] * c[3];
                acc[3] += c[3];
            }
            let src = [acc[0] / 4.0, acc[1] / 4.0, acc[2] / 4.0, acc[3] / 4.0];
            blend_pixel(&mut img.data[i], src, req.blend, req.opacity * cov);
        }
    }
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, bounds) })
}
