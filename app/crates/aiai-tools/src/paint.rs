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
    // Catmull-Rom centrípeta (α = 0,5) amostrada a cada ~1 px (o motor de dab cuida do espaçamento). A uniforme
    // fazia barriga/laço quando um trecho longo vinha seguido de um curto e o traço mudava de forma ao soltar.
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let n = pts.len();
    let get = |i: isize| pts[i.clamp(0, n as isize - 1) as usize];
    let knot = |a: BrushPoint, b: BrushPoint| ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt().sqrt().max(1e-4);
    let mut out = Vec::new();
    for i in 0..n - 1 {
        let (p0, p1, p2, p3) = (get(i as isize - 1), get(i as isize), get(i as isize + 1), get(i as isize + 2));
        let t0 = 0.0f32;
        let t1 = t0 + knot(p0, p1);
        let t2 = t1 + knot(p1, p2);
        let t3 = t2 + knot(p2, p3);
        let len = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let steps = len.ceil().max(1.0) as usize;
        let lerp = |a: (f32, f32), b: (f32, f32), ta: f32, tb: f32, t: f32| {
            let f = if (tb - ta).abs() < 1e-6 { 0.0 } else { (t - ta) / (tb - ta) };
            (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f)
        };
        for k in 0..steps {
            let t = t1 + (t2 - t1) * k as f32 / steps as f32;
            let (q0, q1, q2, q3) = ((p0.x, p0.y), (p1.x, p1.y), (p2.x, p2.y), (p3.x, p3.y));
            let a1 = lerp(q0, q1, t0, t1, t);
            let a2 = lerp(q1, q2, t1, t2, t);
            let a3 = lerp(q2, q3, t2, t3, t);
            let b1 = lerp(a1, a2, t0, t2, t);
            let b2 = lerp(a2, a3, t1, t3, t);
            let c = lerp(b1, b2, t1, t2, t);
            let f = k as f32 / steps as f32;
            out.push(BrushPoint { x: c.0, y: c.1, pressure: p1.pressure + (p2.pressure - p1.pressure) * f });
        }
    }
    out.push(pts[n - 1]);
    out
}

#[cfg(test)]
mod spline_tests {
    use super::*;

    #[test]
    fn centripetal_spline_does_not_overshoot_a_sharp_turn() {
        // trecho longo seguido de um curto com quina: a curva quase não sai da caixa dos pontos
        let p = |x: f32, y: f32| BrushPoint { x, y, pressure: 1.0 };
        let pts = [p(0.0, 0.0), p(200.0, 0.0), p(203.0, 3.0), p(203.0, 200.0)];
        let out = spline_path(&pts);
        let (mx, my) = out.iter().fold((f32::MIN, f32::MIN), |a, q| (a.0.max(q.x), a.1.max(q.y)));
        let mn = out.iter().fold(f32::MAX, |a, q| a.min(q.y));
        // a uniforme passava 12,5 px da quina (x 215,5; y −12,3); a centrípeta fica em ~2,6 px
        assert!(mx <= 206.0 && my <= 200.5 && mn >= -3.0, "sem barriga: max x {mx}, min y {mn}");
        assert!((out[0].x - 0.0).abs() < 1e-3 && (out.last().unwrap().y - 200.0).abs() < 1e-3);
    }
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ShapeSpec {
    /// Contorno fechado: pontos [x, y].
    pub points: Vec<[f32; 2]>,
    /// "spline" (curva suave pelos pontos, padrão) ou "polyline" (lados retos).
    #[serde(default = "d_geom")]
    pub geometry: String,
}

fn d_geom() -> String {
    "spline".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillShapeRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada raster.
    pub layer: Id,
    /// Um ou mais contornos (regra não-zero: um contorno em sentido oposto abre buraco).
    pub shapes: Vec<ShapeSpec>,
    pub color: ColorRef,
    #[serde(default = "d_one_f")]
    pub opacity: f32,
    #[serde(default)]
    pub blend: BlendMode,
    /// Sem antisserrilhado (cor chapada de cel).
    #[serde(default)]
    pub aliased: bool,
    /// Borda irregular (letreiro/pincel): amplitude e comprimento de onda em px.
    #[serde(default)]
    pub roughness: f32,
    #[serde(default = "d_wave_f")]
    pub roughness_wavelength: f32,
    #[serde(default)]
    pub seed: u32,
}

fn d_one_f() -> f32 {
    1.0
}
fn d_wave_f() -> f32 {
    18.0
}

/// Massa de cor: contornos fechados preenchidos numa camada raster (manchas de cor, letreiros, formas de cenário).
pub fn fill_shape_command(p: &Project, blobs: &mut BlobStore, req: &FillShapeRequest) -> ToolResult<Command> {
    let (node, cel, w, h) = raster_cel(p, &req.cut, &req.layer, req.frame)?;
    let mut img = cel_raster(blobs, cel, w, h)?;
    let mut contours: Vec<Vec<[f32; 2]>> = Vec::new();
    for sh in &req.shapes {
        if sh.points.len() < 3 {
            return Err(ToolError::Invalid("contorno precisa de 3+ pontos".into()));
        }
        let pts: Vec<[f32; 2]> = if sh.geometry == "polyline" {
            sh.points.clone()
        } else {
            let stroke = Stroke {
                id: Id::default(),
                points: sh.points.iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
                geometry: StrokeGeometry::Spline,
                tension: 0.0,
                tip: VectorTip::Marker,
                width: 1.0,
                profile: WidthProfile::default(),
                color: ColorRef::default(),
                opacity: 1.0,
                closed: true,
                aliased: false,
                color_trace: false,
            };
            aiai_paint::sample_stroke(&stroke).iter().map(|q| [q.x, q.y]).collect()
        };
        contours.push(pts);
    }
    aiai_paint::text::roughen_contours(&mut contours, req.roughness, req.roughness_wavelength, req.seed);
    let cov = aiai_paint::poly::fill_polygons(&contours, req.aliased, w, h).ok_or_else(|| ToolError::Invalid("forma fora do canvas".into()))?;
    let c = resolve_color(p, &req.color)?;
    let a = c[3] * req.opacity;
    let src = [c[0] * a, c[1] * a, c[2] * a, a];
    for y in 0..cov.height {
        for x in 0..cov.width {
            let v = cov.cov[(y * cov.width + x) as usize];
            if v > 0.0 {
                let i = img.idx(cov.x0 as u32 + x, cov.y0 as u32 + y);
                aiai_paint::blend::blend_pixel(&mut img.data[i], src, req.blend, v);
            }
        }
    }
    let bounds = [cov.x0, cov.y0, cov.x0 + cov.width as i32, cov.y0 + cov.height as i32];
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, bounds) })
}
