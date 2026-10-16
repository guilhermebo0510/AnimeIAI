//! Pintura raster: pincel e degradê no cel exposto de uma camada raster → comando `set_tiles` (só os tiles tocados).

use crate::context::*;
use aiai_core::command::TileEdit;
use aiai_core::*;
use aiai_paint::brush::{BrushPoint, BrushPreset, BrushTip, GradientKind, gradient_fill, paint_stroke};
use aiai_paint::{Image, Mask, tile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PaintStrokeSpec {
    /// Pontos [x, y] ou [x, y, pressão].
    pub points: Vec<Vec<f32>>,
    /// Interpola os pontos como spline suave (recomendado para poucos pontos).
    #[serde(default = "yes")]
    pub smooth: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PaintRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub preset: BrushPreset,
    /// Cor sRGB não pré-multiplicada (ou referência de paleta).
    pub color: ColorRef,
    pub strokes: Vec<PaintStrokeSpec>,
    #[serde(default)]
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GradientRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub kind: GradientKind,
    pub from: [f32; 2],
    pub to: [f32; 2],
    /// Paradas: [t, "#rrggbbaa"]
    pub stops: Vec<(f32, String)>,
    #[serde(default)]
    pub blend: BlendMode,
    #[serde(default = "one")]
    pub opacity: f32,
}

fn one() -> f32 {
    1.0
}

pub fn resolve_color(p: &Project, c: &ColorRef) -> ToolResult<[f32; 4]> {
    match c {
        ColorRef::Direct { rgba } => Ok(rgba.0),
        ColorRef::Palette { palette, color } => match p.node(palette).map(|n| &n.data) {
            Some(NodeKind::Palette(pd)) => pd.resolve(color).map(|c| c.0).ok_or_else(|| ToolError::NotFound(format!("cor {color} na paleta"))),
            _ => Err(ToolError::NotFound(format!("paleta {palette}"))),
        },
    }
}

/// Imagem do canvas inteiro a partir dos tiles do cel.
pub fn cel_raster(blobs: &BlobStore, cel: &CelData, w: u32, h: u32) -> ToolResult<Image> {
    let mut img = Image::new(w, h);
    for t in &cel.tiles {
        let b = blobs.get(&t.blob).ok_or_else(|| ToolError::NotFound(format!("blob do tile {} {}", t.x, t.y)))?;
        let px = tile::decode_tile(&b).map_err(ToolError::Invalid)?;
        tile::blit_tile(&mut img, &px, t.x, t.y, [0, 0]);
    }
    Ok(img)
}

/// Edições de tile para a região `bounds` [x0, y0, x1, y1) da imagem nova.
pub fn tiles_for_region(blobs: &mut BlobStore, img: &Image, bounds: [i32; 4]) -> Vec<TileEdit> {
    let ts = TILE_SIZE as i32;
    let mut edits = Vec::new();
    for ty in bounds[1].div_euclid(ts)..=((bounds[3] - 1).div_euclid(ts)) {
        for tx in bounds[0].div_euclid(ts)..=((bounds[2] - 1).div_euclid(ts)) {
            let mut px = vec![[0.0f32; 4]; tile::TILE_PIXELS];
            let mut any = false;
            for row in 0..ts {
                let y = ty * ts + row;
                if y < 0 || y >= img.height as i32 {
                    continue;
                }
                for col in 0..ts {
                    let x = tx * ts + col;
                    if x < 0 || x >= img.width as i32 {
                        continue;
                    }
                    let p = img.get(x as u32, y as u32);
                    if p[3] > 0.0 {
                        any = true;
                    }
                    px[(row * ts + col) as usize] = p;
                }
            }
            let blob = if any { Some(blobs.put(tile::encode_tile(&px))) } else { None };
            edits.push(TileEdit { x: tx, y: ty, blob });
        }
    }
    edits
}

fn raster_cel<'a>(p: &'a Project, cut_id: &Id, layer: &Id, frame: u32) -> ToolResult<(&'a Node, &'a CelData, u32, u32)> {
    let cut = cut_data(p, cut_id)?;
    let l = layer_data(p, layer)?;
    if l.kind != LayerType::Raster {
        return Err(ToolError::Invalid("pincel raster pinta em camada raster".into()));
    }
    if l.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, layer, frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {frame}")))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let (w, h) = canvas_size(p, cut);
    Ok((node, cel, w, h))
}

fn spline_path(pts: &[BrushPoint]) -> Vec<BrushPoint> {
    // Catmull-Rom uniforme amostrada a cada ~1 px (o motor de dab cuida do espaçamento).
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let n = pts.len();
    let get = |i: isize| pts[i.clamp(0, n as isize - 1) as usize];
    let mut out = Vec::new();
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (get(i as isize - 1), get(i as isize), get(i as isize + 1), get(i as isize + 2));
        let len = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let steps = len.ceil().max(1.0) as usize;
        for k in 0..steps {
            let t = k as f32 / steps as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push(BrushPoint { x: f(p0.x, p1.x, p2.x, p3.x), y: f(p0.y, p1.y, p2.y, p3.y), pressure: p1.pressure + (p2.pressure - p1.pressure) * t });
        }
    }
    out.push(pts[n - 1]);
    out
}

pub fn paint_command(p: &Project, blobs: &mut BlobStore, req: &PaintRequest) -> ToolResult<Command> {
    req.preset.validate().map_err(ToolError::Invalid)?;
    let (node, cel, w, h) = raster_cel(p, &req.cut, &req.layer, req.frame)?;
    let mut img = cel_raster(blobs, cel, w, h)?;
    let color = resolve_color(p, &req.color)?;
    let mut preset = req.preset.clone();
    if let BrushTip::Image { blob, mask } = &mut preset.tip {
        let b = blobs.get(blob).ok_or_else(|| ToolError::NotFound(format!("máscara da ponta {blob}")))?;
        *mask = Some(Mask::decode(&b).map_err(ToolError::Invalid)?);
    }
    let mut bounds = [i32::MAX, i32::MAX, i32::MIN, i32::MIN];
    for (k, s) in req.strokes.iter().enumerate() {
        let pts: Vec<BrushPoint> = s
            .points
            .iter()
            .map(|q| match q.as_slice() {
                [x, y] => Ok(BrushPoint { x: *x, y: *y, pressure: 1.0 }),
                [x, y, pr] => Ok(BrushPoint { x: *x, y: *y, pressure: pr.clamp(0.0, 1.0) }),
                _ => Err(ToolError::Invalid("cada ponto é [x, y] ou [x, y, pressão]".into())),
            })
            .collect::<ToolResult<_>>()?;
        let path = if s.smooth { spline_path(&pts) } else { pts };
        if let Some(r) = paint_stroke(&mut img, &preset, &path, color, req.seed.wrapping_add(k as u64 * 7919)) {
            bounds = [bounds[0].min(r.bounds[0]), bounds[1].min(r.bounds[1]), bounds[2].max(r.bounds[2]), bounds[3].max(r.bounds[3])];
        }
    }
    if bounds[0] >= bounds[2] {
        return Err(ToolError::Invalid("o traço ficou fora do canvas".into()));
    }
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, bounds) })
}

pub fn gradient_command(p: &Project, blobs: &mut BlobStore, req: &GradientRequest) -> ToolResult<Command> {
    let (node, cel, w, h) = raster_cel(p, &req.cut, &req.layer, req.frame)?;
    let mut img = cel_raster(blobs, cel, w, h)?;
    let stops = req
        .stops
        .iter()
        .map(|(t, hex)| Rgba::from_hex(hex).map(|c| (*t, c.0)).ok_or_else(|| ToolError::Invalid(format!("cor inválida {hex}"))))
        .collect::<ToolResult<Vec<_>>>()?;
    gradient_fill(&mut img, req.kind, req.from, req.to, &stops, req.blend, req.opacity);
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]) })
}
