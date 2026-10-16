//! Efeitos 2D de anime desenhados (S14): onomatopeia/texto, linhas de velocidade e de foco, fundos de gag.

use crate::context::*;
use crate::paint::{cel_raster, resolve_color, tiles_for_region};
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use aiai_paint::noise::Rng;
use aiai_paint::poly::fill_polygons;
use aiai_paint::text::{TextOptions, text_outline};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Fonte padrão (Noto Sans JP Regular, OFL) embutida no app.
pub fn default_font() -> &'static [u8] {
    include_bytes!("../../aiai-studio/assets/fonts/NotoSansJP-Regular.otf")
}

/// Fonte pesada (Noto Sans JP Black, OFL) para onomatopeias e títulos.
pub fn heavy_font() -> &'static [u8] {
    include_bytes!("../../aiai-studio/assets/fonts/NotoSansJP-Black.otf")
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextRequest {
    pub cut: Id,
    pub frame: u32,
    /// Camada raster que recebe o preenchimento das letras.
    pub fill_layer: Id,
    /// Camada vetorial opcional para o contorno (traços fechados).
    #[serde(default)]
    pub outline_layer: Option<Id>,
    pub text: String,
    /// Caminho de um arquivo de fonte, ou "regular"/"black" (Noto Sans JP embutida). Omitido = "black".
    #[serde(default)]
    pub font: Option<String>,
    pub size: f32,
    pub center: [f32; 2],
    #[serde(default)]
    pub vertical: bool,
    #[serde(default)]
    pub letter_spacing: f32,
    #[serde(default)]
    pub rotation: f32,
    #[serde(default)]
    pub skew: f32,
    #[serde(default = "unit2")]
    pub scale: [f32; 2],
    pub color: ColorRef,
    #[serde(default)]
    pub outline_width: f32,
    #[serde(default)]
    pub outline_color: Option<ColorRef>,
}

fn unit2() -> [f32; 2] {
    [1.0, 1.0]
}

/// Lote: letras preenchidas (raster) + contorno (vetor), prontas para efeitos (tremor, impacto) por camada.
pub fn text_command(p: &Project, blobs: &mut BlobStore, req: &TextRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let bytes;
    let font: &[u8] = match req.font.as_deref() {
        None | Some("black") => heavy_font(),
        Some("regular") => default_font(),
        Some(path) => {
            bytes = std::fs::read(path).map_err(|e| ToolError::Invalid(format!("fonte {path}: {e}")))?;
            &bytes
        }
    };
    let opt = TextOptions { size: req.size, letter_spacing: req.letter_spacing, vertical: req.vertical, center: req.center, rotation: req.rotation, skew: req.skew, scale: req.scale };
    let outline = text_outline(font, &req.text, &opt).map_err(ToolError::Invalid)?;
    let fl = layer_data(p, &req.fill_layer)?;
    if fl.kind != LayerType::Raster {
        return Err(ToolError::Invalid("fill_layer precisa ser raster".into()));
    }
    let fnode = exposed_cel(p, cut, &req.fill_layer, req.frame).ok_or_else(|| ToolError::Invalid("camada de preenchimento sem cel no quadro".into()))?;
    let NodeKind::Cel(fcel) = &fnode.data else { unreachable!() };
    let mut img = cel_raster(blobs, fcel, w, h)?;
    let color = resolve_color(p, &req.color)?;
    let cov = fill_polygons(&outline.contours, false, w, h).ok_or_else(|| ToolError::Invalid("texto fora do canvas".into()))?;
    let premul = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
    for y in 0..cov.height {
        for x in 0..cov.width {
            let c = cov.cov[(y * cov.width + x) as usize];
            if c > 0.0 {
                let i = img.idx(cov.x0 as u32 + x, cov.y0 as u32 + y);
                aiai_paint::Image::over(&mut img.data[i], premul, c);
            }
        }
    }
    let bounds = [cov.x0, cov.y0, cov.x0 + cov.width as i32, cov.y0 + cov.height as i32];
    let mut cmds = vec![Command::SetTiles { cel: fnode.id.clone(), tiles: tiles_for_region(blobs, &img, bounds) }];
    if let (Some(ol), true) = (&req.outline_layer, req.outline_width > 0.0) {
        let onode = exposed_cel(p, cut, ol, req.frame).ok_or_else(|| ToolError::Invalid("camada de contorno sem cel no quadro".into()))?;
        let NodeKind::Cel(ocel) = &onode.data else { unreachable!() };
        let oc = req.outline_color.clone().unwrap_or(ColorRef::Direct { rgba: Rgba::BLACK });
        let mut idx = ocel.strokes.len();
        let mut items = Vec::new();
        for c in &outline.contours {
            items.push(IndexedStroke {
                index: idx,
                stroke: Stroke {
                    id: Id(String::new()),
                    points: c.iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
                    geometry: StrokeGeometry::Polyline,
                    tension: 0.0,
                    tip: VectorTip::Marker,
                    width: req.outline_width,
                    profile: WidthProfile::default(),
                    color: oc.clone(),
                    opacity: 1.0,
                    closed: true,
                    aliased: false,
                    color_trace: false,
                },
            });
            idx += 1;
        }
        cmds.push(Command::InsertStrokes { cel: onode.id.clone(), items });
    }
    Ok(Command::Batch { commands: cmds, label: Some(format!("Texto \"{}\"", req.text)) })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LinesKind {
    /// Linhas de velocidade paralelas na direção `angle` (graus), dentro de `area`.
    Speed { angle: f32, area: [f32; 4] },
    /// Linhas de foco (集中線) convergindo para `center`, começando fora de uma elipse livre `clear` [rx, ry].
    Focus { center: [f32; 2], clear: [f32; 2] },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LinesRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub kind: LinesKind,
    pub count: u32,
    /// Comprimento (px) mín/máx (linhas de foco: até a borda se maior).
    pub length: [f32; 2],
    /// Largura (px) mín/máx.
    pub width: [f32; 2],
    pub color: ColorRef,
    #[serde(default)]
    pub seed: u64,
}

fn tapered(points: Vec<[f32; 2]>, width: f32, color: &ColorRef, profile: Vec<[f32; 2]>) -> Stroke {
    Stroke {
        id: Id(String::new()),
        points: points.into_iter().map(|q| StrokePoint { x: q[0], y: q[1], pressure: 1.0, tilt: None, t: None }).collect(),
        geometry: StrokeGeometry::Polyline,
        tension: 0.0,
        tip: VectorTip::Marker,
        width,
        profile: WidthProfile(profile),
        color: color.clone(),
        opacity: 1.0,
        closed: false,
        aliased: false,
        color_trace: false,
    }
}

/// Linhas de velocidade/foco como traços afinados (vetor: dá para editar depois e trocar a semente por quadro para "ferver").
pub fn lines_command(p: &Project, req: &LinesRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid("camada sem cel no quadro".into()))?;
    let NodeKind::Cel(cel) = &node.data else { unreachable!() };
    let mut rng = Rng::new(req.seed ^ 0x5157);
    let mut items = Vec::new();
    let mut idx = cel.strokes.len();
    let lw = |rng: &mut Rng| req.width[0] + (req.width[1] - req.width[0]) * rng.f32();
    let ll = |rng: &mut Rng| req.length[0] + (req.length[1] - req.length[0]) * rng.f32();
    for _ in 0..req.count.min(2000) {
        let s = match &req.kind {
            LinesKind::Speed { angle, area } => {
                let (sn, cs) = angle.to_radians().sin_cos();
                let (cx, cy) = (area[0] + rng.f32() * area[2], area[1] + rng.f32() * area[3]);
                let len = ll(&mut rng);
                let a = [cx - cs * len / 2.0, cy - sn * len / 2.0];
                let b = [cx + cs * len / 2.0, cy + sn * len / 2.0];
                tapered(vec![a, b], lw(&mut rng), &req.color, vec![[0.0, 0.0], [0.3, 1.0], [0.7, 1.0], [1.0, 0.0]])
            }
            LinesKind::Focus { center, clear } => {
                let a = rng.f32() * std::f32::consts::TAU;
                let (dx, dy) = (a.cos(), a.sin());
                // início na borda da elipse livre (com variação), fim fora do canvas
                let r_in = 1.0 / ((dx / clear[0].max(1.0)).powi(2) + (dy / clear[1].max(1.0)).powi(2)).sqrt();
                let start = r_in * (1.0 + 0.25 * rng.f32());
                let reach = ((w * w + h * h) as f32).sqrt();
                let end = (start + ll(&mut rng)).min(reach);
                let p0 = [center[0] + dx * start, center[1] + dy * start];
                let p1 = [center[0] + dx * end, center[1] + dy * end];
                tapered(vec![p0, p1], lw(&mut rng), &req.color, vec![[0.0, 0.0], [0.6, 0.7], [1.0, 1.0]])
            }
        };
        items.push(IndexedStroke { index: idx, stroke: s });
        idx += 1;
    }
    Ok(Command::InsertStrokes { cel: node.id.clone(), items })
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GagPattern {
    /// Faixas radiais alternadas (fundo de choque/alegria).
    RadialStripes { center: [f32; 2], count: u32, colors: [String; 2] },
    /// Retícula de pontos (screentone).
    Dots { spacing: f32, radius: f32, color: String, background: String, #[serde(default)] angle: f32 },
    /// Listras diagonais.
    Stripes { spacing: f32, width: f32, angle: f32, colors: [String; 2] },
    Flat { color: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GagBgRequest {
    pub cut: Id,
    pub frame: u32,
    pub layer: Id,
    pub pattern: GagPattern,
}

pub fn gag_bg_command(p: &Project, blobs: &mut BlobStore, req: &GagBgRequest) -> ToolResult<Command> {
    let cut = cut_data(p, &req.cut)?;
    let (w, h) = canvas_size(p, cut);
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid("camada sem cel no quadro".into()))?;
    let hex = |s: &str| Rgba::from_hex(s).map(|c| c.0).ok_or_else(|| ToolError::Invalid(format!("cor inválida {s}")));
    let mut img = aiai_paint::Image::new(w, h);
    match &req.pattern {
        GagPattern::Flat { color } => {
            let c = hex(color)?;
            img = aiai_paint::Image::filled(w, h, c);
        }
        GagPattern::RadialStripes { center, count, colors } => {
            let (a, b) = (hex(&colors[0])?, hex(&colors[1])?);
            let n = (*count).max(2) as f32;
            for y in 0..h {
                for x in 0..w {
                    let ang = (y as f32 + 0.5 - center[1]).atan2(x as f32 + 0.5 - center[0]);
                    let f = ((ang / std::f32::consts::TAU + 0.5) * n).fract();
                    // borda suave de ~1 px no ângulo
                    let r = ((x as f32 - center[0]).powi(2) + (y as f32 - center[1]).powi(2)).sqrt().max(1.0);
                    let edge_w = n / (std::f32::consts::TAU * r);
                    let t = ((f - 0.5).abs() * 2.0 - 0.5) / edge_w.max(1e-3) * 0.5 + 0.5;
                    let k = t.clamp(0.0, 1.0);
                    let c = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k, 1.0];
                    let i = img.idx(x, y);
                    img.data[i] = c;
                }
            }
        }
        GagPattern::Dots { spacing, radius, color, background, angle } => {
            let (fg, bg) = (hex(color)?, hex(background)?);
            let (sn, cs) = angle.to_radians().sin_cos();
            let s = spacing.max(2.0);
            for y in 0..h {
                for x in 0..w {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let (u, v) = (px * cs + py * sn, -px * sn + py * cs);
                    let (fu, fv) = ((u / s).fract() - 0.5, (v / s).fract() - 0.5);
                    let d = (fu * fu + fv * fv).sqrt() * s;
                    let k = (radius - d + 0.5).clamp(0.0, 1.0);
                    let i = img.idx(x, y);
                    img.data[i] = [bg[0] + (fg[0] - bg[0]) * k, bg[1] + (fg[1] - bg[1]) * k, bg[2] + (fg[2] - bg[2]) * k, 1.0];
                }
            }
        }
        GagPattern::Stripes { spacing, width, angle, colors } => {
            let (a, b) = (hex(&colors[0])?, hex(&colors[1])?);
            let (sn, cs) = angle.to_radians().sin_cos();
            for y in 0..h {
                for x in 0..w {
                    let u = (x as f32 + 0.5) * cs + (y as f32 + 0.5) * sn;
                    let f = u.rem_euclid(spacing.max(1.0));
                    let k = ((width - f + 0.5).clamp(0.0, 1.0)).max(((f - (spacing - 0.5)).clamp(0.0, 1.0)) * 0.0);
                    let i = img.idx(x, y);
                    img.data[i] = [b[0] + (a[0] - b[0]) * k, b[1] + (a[1] - b[1]) * k, b[2] + (a[2] - b[2]) * k, 1.0];
                }
            }
        }
    }
    Ok(Command::SetTiles { cel: node.id.clone(), tiles: tiles_for_region(blobs, &img, [0, 0, w as i32, h as i32]) })
}
