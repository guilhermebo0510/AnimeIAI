//! Cenário: céu, preenchimento por textura (plano ou em perspectiva por homografia) numa região.

use crate::context::*;
use crate::paint::{cel_raster, tiles_for_region};
use aiai_core::*;
use aiai_paint::blend::blend_pixel;
use aiai_paint::select::{SelectShape, Selection};
use aiai_paint::texture::{SkySpec, TextureParams};
use aiai_paint::warp::Homography;
use rayon::prelude::*;
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
    // Amostragem adaptativa: quantos texels um pixel cobre (derivada do mapeamento) → n×n amostras (2..8).
    // Evita moiré do padrão ao longe sem custo onde o padrão é grande.
    let tex_uv = |x: f32, y: f32| -> (f32, f32) {
        match &map {
            Map::Flat { inv_s, cos, sin, off } => {
                let (dx, dy) = (x - off[0], y - off[1]);
                ((dx * cos - dy * sin) * inv_s, (dx * sin + dy * cos) * inv_s)
            }
            Map::Persp { inv, size } => {
                let (u, v) = inv.apply(x, y);
                (u * size[0], v * size[1])
            }
        }
    };
    let rows: Vec<(i32, Vec<(usize, [f32; 4])>)> = (bounds[1]..bounds[3])
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|y| {
            let mut out = Vec::new();
            for x in bounds[0]..bounds[2] {
                let i = (y as u32 * w + x as u32) as usize;
                if sel.cov[i] <= 0.0 {
                    continue;
                }
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let (u0, v0) = tex_uv(cx, cy);
                let (u1, v1) = tex_uv(cx + 1.0, cy);
                let (u2, v2) = tex_uv(cx, cy + 1.0);
                let foot = ((u1 - u0).hypot(v1 - v0)).max((u2 - u0).hypot(v2 - v0));
                let n = (foot / 1.5).ceil().clamp(2.0, 8.0) as usize;
                let mut acc = [0.0f32; 4];
                for sy in 0..n {
                    for sx in 0..n {
                        let c = tex_at(x as f32 + (sx as f32 + 0.5) / n as f32, y as f32 + (sy as f32 + 0.5) / n as f32);
                        acc[0] += c[0] * c[3];
                        acc[1] += c[1] * c[3];
                        acc[2] += c[2] * c[3];
                        acc[3] += c[3];
                    }
                }
                let k = 1.0 / (n * n) as f32;
                out.push((i, [acc[0] * k, acc[1] * k, acc[2] * k, acc[3] * k]));
            }
            (y, out)
        })
        .collect();
    for (_, row) in rows {
        for (i, src) in row {
            blend_pixel(&mut img.data[i], src, req.blend, req.opacity * sel.cov[i]);
        }
    }
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, bounds) })
}
