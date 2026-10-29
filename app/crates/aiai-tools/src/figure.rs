//! Figura (S25, à moda do Clip Studio): formas diretas — reta, curva de dois passos, polilinha, curva contínua, Bézier,
//! retângulo (cantos arredondados), elipse, polígono e laço de preenchimento — com linha, preenchimento ou os dois, em
//! qualquer camada de desenho:
//! - vetorial: a linha é um traço editável (Corrigir linha mexe nos pontos e nas alças) e o preenchimento uma região de
//!   preenchimento do cel (máscara), pintada embaixo dos traços;
//! - raster: linha e preenchimento viram pixels (a linha com o mesmo rasterizador dos traços vetoriais), só dentro da
//!   seleção quando há uma;
//! - preenchimento: tudo vira região de preenchimento (a linha também, pela cobertura do traço).
//!
//! A geometria (`FigureShape::outline`) é a mesma que o app mostra na prévia. As linhas de velocidade e de foco da Figura
//! usam `fx2d::lines_command` (ver `lines_layer_command`: refazer numa camada de linhas troca os traços dela).

use crate::context::*;
use crate::fx2d::{LinesKind, LinesRequest, lines_command};
use aiai_core::command::IndexedStroke;
use aiai_core::*;
use aiai_paint::select::SelectShape;
use aiai_paint::{Image, Mask, rasterize_stroke, sample_stroke};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// O que a forma cria (Clip Studio: "Criar linha", "Criar preenchimento", "Criar linha e preenchimento").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FigureMode {
    #[default]
    Line,
    Fill,
    LineFill,
}

impl FigureMode {
    pub fn line(self) -> bool {
        matches!(self, FigureMode::Line | FigureMode::LineFill)
    }
    pub fn fill(self) -> bool {
        matches!(self, FigureMode::Fill | FigureMode::LineFill)
    }
}

/// Ponto da curva de Bézier: `handle` = alça de saída (a curva sai por `at + handle` e chega por `at - handle`); sem
/// alça, o ponto é canto (clique sem arrastar no Clip Studio).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BezierAnchor {
    pub at: [f32; 2],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handle: Option<[f32; 2]>,
}

/// Forma da Figura (px do canvas, y para baixo).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FigureShape {
    /// Reta.
    Line { from: [f32; 2], to: [f32; 2] },
    /// Curva de dois passos do Clip Studio: a reta de `from` a `to` puxada até passar por `through`.
    Curve { from: [f32; 2], to: [f32; 2], through: [f32; 2] },
    /// Polilinha (lados retos); `closed` fecha no primeiro ponto.
    Polyline {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        closed: bool,
    },
    /// Curva contínua: spline suave pelos pontos.
    ContinuousCurve {
        points: Vec<[f32; 2]>,
        #[serde(default)]
        closed: bool,
    },
    /// Curva de Bézier por pontos com alças.
    Bezier {
        anchors: Vec<BezierAnchor>,
        #[serde(default)]
        closed: bool,
    },
    /// Retângulo [x, y, w, h] com cantos arredondados de raio `radius` (px) e giro `angle` (graus, horário, em volta do
    /// centro).
    Rect {
        rect: [f32; 4],
        #[serde(default)]
        radius: f32,
        #[serde(default)]
        angle: f32,
    },
    /// Elipse inscrita em [x, y, w, h], girada `angle` graus.
    Ellipse {
        rect: [f32; 4],
        #[serde(default)]
        angle: f32,
    },
    /// Polígono regular de `sides` lados com o primeiro vértice para cima, raios [rx, ry], girado `angle` graus.
    Polygon {
        center: [f32; 2],
        radius: [f32; 2],
        sides: u32,
        #[serde(default)]
        angle: f32,
    },
    /// Laço de preenchimento: o contorno desenhado à mão, sempre preenchido.
    Lasso { points: Vec<[f32; 2]> },
}

/// Contorno da forma como traço vetorial: pontos (com canto e alça), geometria e se fecha.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub points: Vec<StrokePoint>,
    pub geometry: StrokeGeometry,
    pub closed: bool,
}

fn sp(p: [f32; 2]) -> StrokePoint {
    StrokePoint { x: p[0], y: p[1], pressure: 1.0, tilt: None, t: None, corner: false, width: 1.0, id: 0, handle: None }
}

fn rot(p: [f32; 2], c: [f32; 2], deg: f32) -> [f32; 2] {
    if deg == 0.0 {
        return p;
    }
    let (s, k) = deg.to_radians().sin_cos();
    let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
    [c[0] + dx * k - dy * s, c[1] + dx * s + dy * k]
}

fn rot_v(v: [f32; 2], deg: f32) -> [f32; 2] {
    rot(v, [0.0, 0.0], deg)
}

/// Caixa [x, y, w, h] normalizada (largura/altura positivas).
fn norm_rect(r: [f32; 4]) -> [f32; 4] {
    let (x0, x1) = (r[0].min(r[0] + r[2]), r[0].max(r[0] + r[2]));
    let (y0, y1) = (r[1].min(r[1] + r[3]), r[1].max(r[1] + r[3]));
    [x0, y0, x1 - x0, y1 - y0]
}

/// Constante da cúbica que aproxima o quarto de círculo.
const KAPPA: f32 = 0.552_284_8;

impl FigureShape {
    /// A forma fecha (contorno de área: preenchimento possível).
    pub fn closed(&self) -> bool {
        match self {
            FigureShape::Line { .. } | FigureShape::Curve { .. } => false,
            FigureShape::Polyline { closed, .. } | FigureShape::ContinuousCurve { closed, .. } | FigureShape::Bezier { closed, .. } => *closed,
            FigureShape::Rect { .. } | FigureShape::Ellipse { .. } | FigureShape::Polygon { .. } | FigureShape::Lasso { .. } => true,
        }
    }

    /// Contorno como traço (pontos editáveis depois: a curva e a Bézier guardam as alças nos pontos).
    pub fn outline(&self) -> ToolResult<Outline> {
        let fin = |v: &[f32]| v.iter().all(|x| x.is_finite());
        let bad = |m: &str| Err(ToolError::Invalid(m.into()));
        let o = match self {
            FigureShape::Line { from, to } => {
                if !fin(from) || !fin(to) {
                    return bad("reta com coordenada inválida");
                }
                Outline { points: vec![sp(*from), sp(*to)], geometry: StrokeGeometry::Polyline, closed: false }
            }
            FigureShape::Curve { from, to, through } => {
                if !fin(from) || !fin(to) || !fin(through) {
                    return bad("curva com coordenada inválida");
                }
                // quadrática que passa por `through` no meio (t = 0,5): controle q = 2m - (a + b)/2; elevada a cúbica,
                // as alças são 2/3 do caminho até q
                let q = [2.0 * through[0] - (from[0] + to[0]) / 2.0, 2.0 * through[1] - (from[1] + to[1]) / 2.0];
                let mut a = sp(*from);
                a.handle = Some([(q[0] - from[0]) * 2.0 / 3.0, (q[1] - from[1]) * 2.0 / 3.0]);
                let mut b = sp(*to);
                b.handle = Some([(to[0] - q[0]) * 2.0 / 3.0, (to[1] - q[1]) * 2.0 / 3.0]);
                Outline { points: vec![a, b], geometry: StrokeGeometry::Spline, closed: false }
            }
            FigureShape::Polyline { points, closed } => {
                if points.len() < 2 || !points.iter().all(|p| fin(p)) {
                    return bad("polilinha precisa de 2+ pontos válidos");
                }
                Outline { points: points.iter().map(|p| sp(*p)).collect(), geometry: StrokeGeometry::Polyline, closed: *closed && points.len() >= 3 }
            }
            FigureShape::ContinuousCurve { points, closed } => {
                if points.len() < 2 || !points.iter().all(|p| fin(p)) {
                    return bad("curva contínua precisa de 2+ pontos válidos");
                }
                Outline { points: points.iter().map(|p| sp(*p)).collect(), geometry: StrokeGeometry::Spline, closed: *closed && points.len() >= 2 }
            }
            FigureShape::Bezier { anchors, closed } => {
                if anchors.len() < 2 || !anchors.iter().all(|a| fin(&a.at) && a.handle.is_none_or(|h| fin(&h))) {
                    return bad("Bézier precisa de 2+ pontos válidos");
                }
                let points = anchors
                    .iter()
                    .map(|a| {
                        let mut q = sp(a.at);
                        match a.handle.filter(|h| h[0].hypot(h[1]) > 1e-3) {
                            Some(h) => q.handle = Some(h),
                            None => q.corner = true,
                        }
                        q
                    })
                    .collect();
                Outline { points, geometry: StrokeGeometry::Spline, closed: *closed }
            }
            FigureShape::Rect { rect, radius, angle } => {
                if !fin(rect) || !radius.is_finite() || !angle.is_finite() {
                    return bad("retângulo com valor inválido");
                }
                let [x, y, w, h] = norm_rect(*rect);
                if w < 0.5 || h < 0.5 {
                    return bad("retângulo vazio");
                }
                let c = [x + w / 2.0, y + h / 2.0];
                let r = radius.max(0.0).min(w.min(h) / 2.0);
                let mut pts: Vec<[f32; 2]> = Vec::new();
                if r < 0.5 {
                    pts = vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]];
                } else {
                    // cantos em arco (8 trechos por canto), no sentido horário a partir do canto de cima à esquerda
                    let corners = [([x + r, y + r], 180.0f32), ([x + w - r, y + r], 270.0), ([x + w - r, y + h - r], 0.0), ([x + r, y + h - r], 90.0)];
                    for (cc, a0) in corners {
                        for k in 0..=8 {
                            let a = (a0 + 90.0 * k as f32 / 8.0).to_radians();
                            pts.push([cc[0] + r * a.cos(), cc[1] + r * a.sin()]);
                        }
                    }
                }
                let points = pts.into_iter().map(|p| sp(rot(p, c, *angle))).collect();
                Outline { points, geometry: StrokeGeometry::Polyline, closed: true }
            }
            FigureShape::Ellipse { rect, angle } => {
                if !fin(rect) || !angle.is_finite() {
                    return bad("elipse com valor inválido");
                }
                let [x, y, w, h] = norm_rect(*rect);
                if w < 0.5 || h < 0.5 {
                    return bad("elipse vazia");
                }
                let (c, rx, ry) = ([x + w / 2.0, y + h / 2.0], w / 2.0, h / 2.0);
                // quatro pontos com alças (a cúbica do quarto de círculo): editável como uma elipse do Illustrator
                let quads = [([c[0] + rx, c[1]], [0.0, KAPPA * ry]), ([c[0], c[1] + ry], [-KAPPA * rx, 0.0]), ([c[0] - rx, c[1]], [0.0, -KAPPA * ry]), ([c[0], c[1] - ry], [KAPPA * rx, 0.0])];
                let points = quads
                    .iter()
                    .map(|(p, h)| {
                        let mut q = sp(rot(*p, c, *angle));
                        q.handle = Some(rot_v(*h, *angle));
                        q
                    })
                    .collect();
                Outline { points, geometry: StrokeGeometry::Spline, closed: true }
            }
            FigureShape::Polygon { center, radius, sides, angle } => {
                if !fin(center) || !fin(radius) || !angle.is_finite() {
                    return bad("polígono com valor inválido");
                }
                if !(3..=64).contains(sides) {
                    return bad("polígono precisa de 3 a 64 lados");
                }
                if radius[0].abs() < 0.5 || radius[1].abs() < 0.5 {
                    return bad("polígono vazio");
                }
                let n = *sides as usize;
                let points = (0..n)
                    .map(|i| {
                        let a = (-90.0f32 + 360.0 * i as f32 / n as f32).to_radians();
                        sp(rot([center[0] + radius[0].abs() * a.cos(), center[1] + radius[1].abs() * a.sin()], *center, *angle))
                    })
                    .collect();
                Outline { points, geometry: StrokeGeometry::Polyline, closed: true }
            }
            FigureShape::Lasso { points } => {
                if points.len() < 3 || !points.iter().all(|p| fin(p)) {
                    return bad("laço precisa de 3+ pontos válidos");
                }
                Outline { points: points.iter().map(|p| sp(*p)).collect(), geometry: StrokeGeometry::Polyline, closed: true }
            }
        };
        Ok(o)
    }
}

fn d_width() -> f32 {
    3.0
}
fn one() -> f32 {
    1.0
}
fn d_tip() -> VectorTip {
    VectorTip::Marker
}

/// Pedido da Figura (`aiai_figure`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FigureRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada vetorial, raster ou de preenchimento.
    pub layer: Id,
    pub shape: FigureShape,
    /// line | fill | line_fill (formas abertas só têm linha; o laço sempre preenche).
    #[serde(default)]
    pub mode: FigureMode,
    /// Largura da linha (px).
    #[serde(default = "d_width")]
    pub width: f32,
    /// Cor da linha (e do preenchimento, sem `fill_color`).
    pub color: ColorRef,
    /// Cor do preenchimento (o Clip Studio pinta o preenchimento de "linha e preenchimento" com a cor secundária).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill_color: Option<ColorRef>,
    /// Ponta do traço vetorial (padrão marker: largura constante, como a linha da Figura).
    #[serde(default = "d_tip")]
    pub tip: VectorTip,
    #[serde(default = "one")]
    pub opacity: f32,
    /// Sem antisserrilhado.
    #[serde(default)]
    pub aliased: bool,
    /// Camada raster: só dentro da seleção.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<SelectShape>,
}

/// Traço vetorial do contorno (pontos já no espaço do cel).
pub fn outline_stroke(o: &Outline, width: f32, tip: VectorTip, color: &ColorRef, opacity: f32, aliased: bool) -> Stroke {
    Stroke { taper: None,
        id: Id(String::new()),
        points: o.points.clone(),
        geometry: o.geometry,
        tension: 0.0,
        tip,
        width: width.max(0.1),
        profile: WidthProfile::default(),
        color: color.clone(),
        opacity: opacity.clamp(0.0, 1.0),
        closed: o.closed,
        aliased,
        color_trace: false,
        group: String::new(),
    }
}

/// Contorno achatado (px) para preencher.
pub fn outline_polygon(o: &Outline) -> Vec<[f32; 2]> {
    let s = outline_stroke(o, 1.0, VectorTip::Marker, &ColorRef::default(), 1.0, false);
    sample_stroke(&s).iter().map(|q| [q.x, q.y]).collect()
}

/// Cobertura (canvas inteiro) numa janela: (x0, y0, w, h, cobertura).
struct Cov {
    x0: i32,
    y0: i32,
    w: u32,
    h: u32,
    cov: Vec<f32>,
}

fn fill_cov(o: &Outline, aliased: bool, w: u32, h: u32) -> Option<Cov> {
    let poly = outline_polygon(o);
    let c = aiai_paint::poly::fill_polygons(&[poly], aliased, w, h)?;
    Some(Cov { x0: c.x0, y0: c.y0, w: c.width, h: c.height, cov: c.cov })
}

fn line_cov(stroke: &Stroke, w: u32, h: u32) -> Option<Cov> {
    let c = rasterize_stroke(&sample_stroke(stroke), stroke.aliased, w, h)?;
    Some(Cov { x0: c.x0, y0: c.y0, w: c.width, h: c.height, cov: c.cov })
}

/// Região de preenchimento com a máscara da cobertura.
fn region(blobs: &mut BlobStore, c: &Cov, color: &ColorRef) -> Option<FillRegion> {
    if c.cov.iter().all(|v| *v <= 0.0) {
        return None;
    }
    let mask = Mask::from_coverage(c.w, c.h, &c.cov);
    let hash = blobs.put(mask.encode());
    let seed = [c.x0 as f32 + c.w as f32 / 2.0, c.y0 as f32 + c.h as f32 / 2.0];
    Some(FillRegion { id: Id(String::new()), seed, color: color.clone(), mask: Some(hash), bounds: [c.x0, c.y0, c.w as i32, c.h as i32], gradient: None })
}

/// Pinta a cobertura na imagem (cor pré-multiplicada pela opacidade).
fn blend_cov(img: &mut Image, c: &Cov, rgba: [f32; 4], opacity: f32) -> [i32; 4] {
    let a = rgba[3] * opacity;
    let src = [rgba[0] * a, rgba[1] * a, rgba[2] * a, a];
    for y in 0..c.h {
        for x in 0..c.w {
            let v = c.cov[(y * c.w + x) as usize];
            if v > 0.0 {
                let i = img.idx(c.x0 as u32 + x, c.y0 as u32 + y);
                aiai_paint::blend::blend_pixel(&mut img.data[i], src, BlendMode::Normal, v);
            }
        }
    }
    [c.x0, c.y0, c.x0 + c.w as i32, c.y0 + c.h as i32]
}

fn union(a: Option<[i32; 4]>, b: [i32; 4]) -> [i32; 4] {
    match a {
        None => b,
        Some(a) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
    }
}

/// Leva o contorno do canvas para o espaço do cel (camada com movimento: a forma fica embaixo do cursor).
fn to_cel(p: &Project, req: &FigureRequest, o: &mut Outline) -> ToolResult<()> {
    let place = crate::points::cel_placement(p, &req.cut, &req.layer, req.frame)?;
    if place.0 == aiai_paint::Affine::IDENTITY.0 {
        return Ok(());
    }
    let inv = place.inverse().ok_or_else(|| ToolError::Invalid("a camada tem escala zero neste quadro".into()))?;
    let k = inv.0[0].hypot(inv.0[3]);
    let lin = |h: [f32; 2]| [inv.0[0] * h[0] + inv.0[1] * h[1], inv.0[3] * h[0] + inv.0[4] * h[1]];
    for q in &mut o.points {
        let (x, y) = inv.apply(q.x, q.y);
        (q.x, q.y) = (x, y);
        q.handle = q.handle.map(lin);
    }
    let _ = k;
    Ok(())
}

/// A Figura: comando que cria a forma na camada (um passo de desfazer).
pub fn figure_command(p: &Project, blobs: &mut BlobStore, req: &FigureRequest) -> ToolResult<Command> {
    let mask = match &req.selection {
        Some(shape) => Some(crate::paint::selection_mask(p, blobs, &req.cut, &req.layer, req.frame, shape)?),
        None => None,
    };
    figure_command_masked(p, blobs, req, mask.as_ref())
}

/// Como [`figure_command`], com a seleção já como cobertura do canvas `(cobertura, largura, altura)` (o app).
pub fn figure_command_masked(p: &Project, blobs: &mut BlobStore, req: &FigureRequest, mask: Option<&(Vec<f32>, u32, u32)>) -> ToolResult<Command> {
    if !req.width.is_finite() || req.width <= 0.0 || req.width > 2000.0 {
        return Err(ToolError::Invalid("largura da linha fora de (0, 2000] px".into()));
    }
    if !(0.0..=1.0).contains(&req.opacity) {
        return Err(ToolError::Invalid("opacidade fora de 0..1".into()));
    }
    let cut = cut_data(p, &req.cut)?;
    let l = layer_data(p, &req.layer)?;
    if l.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &node.data else { return Err(ToolError::Invalid("não é cel".into())) };
    let (w, h) = canvas_size(p, cut);
    let lasso = matches!(req.shape, FigureShape::Lasso { .. });
    let mode = if lasso { FigureMode::Fill } else { req.mode };
    let closed = req.shape.closed();
    let (do_line, do_fill) = (mode.line() || !closed, mode.fill() && closed);
    let mut o = req.shape.outline()?;
    let line_color = req.color.clone();
    let fill_color = req.fill_color.clone().unwrap_or_else(|| req.color.clone());
    match l.kind {
        LayerType::Vector | LayerType::Fill => {
            to_cel(p, req, &mut o)?;
            let stroke = outline_stroke(&o, req.width, req.tip, &line_color, req.opacity, req.aliased);
            let mut cmds = Vec::new();
            let mut fills = cel.fills.clone();
            let before = fills.len();
            if do_fill {
                let c = fill_cov(&o, req.aliased, w, h).ok_or_else(|| ToolError::Invalid("forma fora do canvas".into()))?;
                fills.extend(region(blobs, &c, &fill_color));
            }
            if do_line && l.kind == LayerType::Fill {
                // camada de preenchimento não guarda traço: a linha vira região pela cobertura dela
                let c = line_cov(&stroke, w, h).ok_or_else(|| ToolError::Invalid("linha fora do canvas".into()))?;
                fills.extend(region(blobs, &c, &line_color));
            }
            if fills.len() != before {
                cmds.push(Command::ReplaceFills { cel: node.id.clone(), fills });
            }
            if do_line && l.kind == LayerType::Vector {
                cmds.push(Command::InsertStrokes { cel: node.id.clone(), items: vec![IndexedStroke { index: cel.strokes.len(), stroke }] });
            }
            match cmds.len() {
                0 => Err(ToolError::Invalid("a forma ficou fora do canvas".into())),
                1 => Ok(cmds.pop().expect("um")),
                _ => Ok(Command::Batch { commands: cmds, label: Some("Figura".into()) }),
            }
        }
        LayerType::Raster => {
            to_cel(p, req, &mut o)?;
            let mut img = crate::paint::cel_raster(blobs, cel, w, h)?;
            let before = mask.map(|_| img.clone());
            let mut bounds: Option<[i32; 4]> = None;
            if do_fill {
                if let Some(c) = fill_cov(&o, req.aliased, w, h) {
                    let rgba = crate::paint::resolve_color(p, &fill_color)?;
                    bounds = Some(union(bounds, blend_cov(&mut img, &c, rgba, req.opacity)));
                }
            }
            if do_line {
                let stroke = outline_stroke(&o, req.width, req.tip, &line_color, 1.0, req.aliased);
                if let Some(c) = line_cov(&stroke, w, h) {
                    let rgba = crate::paint::resolve_color(p, &line_color)?;
                    bounds = Some(union(bounds, blend_cov(&mut img, &c, rgba, req.opacity)));
                }
            }
            let bounds = bounds.ok_or_else(|| ToolError::Invalid("a forma ficou fora do canvas".into()))?;
            if let (Some(m), Some(b)) = (mask, &before) {
                clip_to_mask(&mut img, b, m, bounds);
            }
            Ok(Command::SetTiles { cel: node.id.clone(), tiles: crate::paint::tiles_for_region(blobs, &img, bounds) })
        }
        other => Err(ToolError::Invalid(format!("a Figura desenha em camada vetorial, raster ou de preenchimento (esta é {other:?})"))),
    }
}

/// Fora da seleção a imagem volta à de antes; na borda suave, mistura pela cobertura.
pub fn clip_to_mask(img: &mut Image, before: &Image, mask: &(Vec<f32>, u32, u32), bounds: [i32; 4]) {
    let (cov, mw, mh) = (&mask.0, mask.1 as i32, mask.2 as i32);
    for y in bounds[1].max(0)..bounds[3].min(img.height as i32) {
        for x in bounds[0].max(0)..bounds[2].min(img.width as i32) {
            let k = if x < mw && y < mh { cov[(y * mw + x) as usize] } else { 0.0 };
            if k >= 1.0 {
                continue;
            }
            let i = img.idx(x as u32, y as u32);
            let (a, b) = (img.data[i], before.data[i]);
            img.data[i] = [0, 1, 2, 3].map(|c| b[c] + (a[c] - b[c]) * k);
        }
    }
}

// ------------------------------------------------------------------------------------------------ linhas de velocidade/foco

/// Chave em `params` da camada de linhas criada pela Figura (os parâmetros para refazer com outro centro/direção).
pub const LINES_PARAM: &str = "figure_lines";

/// Parâmetros de uma camada de linhas guardados nela (`params.figure_lines`).
pub fn lines_params(l: &LayerData) -> Option<LinesRequest> {
    serde_json::from_value(l.params.get(LINES_PARAM)?.clone()).ok()
}

/// Linhas de velocidade/foco na camada vetorial `req.layer`: com `replace`, os traços do cel são trocados pelos novos
/// (refazer as linhas com outro centro, direção ou quantidade); os parâmetros ficam em `params.figure_lines` da camada.
pub fn lines_layer_command(p: &Project, req: &LinesRequest, replace: bool) -> ToolResult<Command> {
    let l = layer_data(p, &req.layer)?;
    if l.kind != LayerType::Vector {
        return Err(ToolError::Invalid("linhas de velocidade/foco vão numa camada vetorial".into()));
    }
    validate_lines(req)?;
    let Command::InsertStrokes { cel, items } = lines_command(p, req)? else { return Err(ToolError::Invalid("linhas: comando inesperado".into())) };
    let mut cmds = Vec::new();
    let mut items = items;
    if replace {
        // refazer: tira os traços que o cel tem e põe os novos a partir do índice 0
        let old: Vec<Id> = match &node(p, &cel)?.data {
            NodeKind::Cel(c) => c.strokes.iter().map(|s| s.id.clone()).collect(),
            _ => Vec::new(),
        };
        if !old.is_empty() {
            cmds.push(Command::RemoveStrokes { cel: cel.clone(), ids: old });
        }
        for (i, it) in items.iter_mut().enumerate() {
            it.index = i;
        }
    }
    cmds.push(Command::InsertStrokes { cel, items });
    let mut params = if l.params.is_object() { l.params.clone() } else { json!({}) };
    params[LINES_PARAM] = serde_json::to_value(req).map_err(|e| ToolError::Invalid(e.to_string()))?;
    let props = Command::SetProps { id: req.layer.clone(), name: None, data: json!({ "params": params }) };
    let label = match req.kind {
        LinesKind::Speed { .. } => "Linhas de velocidade",
        LinesKind::Focus { .. } => "Linhas de foco",
    };
    cmds.push(props);
    Ok(Command::Batch { commands: cmds, label: Some(label.into()) })
}

fn validate_lines(req: &LinesRequest) -> ToolResult<()> {
    let ok = |a: [f32; 2]| a.iter().all(|v| v.is_finite() && *v >= 0.0) && a[0] <= a[1];
    if req.count == 0 || req.count > 2000 {
        return Err(ToolError::Invalid("quantidade de linhas fora de 1..2000".into()));
    }
    if !ok(req.length) || !ok(req.width) {
        return Err(ToolError::Invalid("comprimento/largura das linhas: [mín, máx] com mín ≤ máx, ≥ 0".into()));
    }
    Ok(())
}

/// Valor só para quem lê a camada (bridge/painel): os parâmetros das linhas, se a camada é de linhas da Figura.
pub fn lines_info(l: &LayerData) -> Option<Value> {
    l.params.get(LINES_PARAM).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> (CoreSession, Id, Id, Id, Id) {
        let mut s = CoreSession::for_tests("figure");
        s.project.settings.width = 200;
        s.project.settings.height = 120;
        let mk = |s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind| {
            let id = s.ids.next();
            s.apply(Command::CreateNode { id: Some(id.clone()), parent: parent.cloned(), index: None, name: name.into(), data }).unwrap();
            id
        };
        let ep = mk(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = mk(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = mk(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(4)));
        let mut layers = Vec::new();
        for kind in [LayerType::Vector, LayerType::Raster, LayerType::Fill] {
            let l = mk(&mut s, Some(&cut), "L", NodeKind::Layer(LayerData::new(kind)));
            mk(&mut s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
            layers.push(l);
        }
        (s, cut, layers[0].clone(), layers[1].clone(), layers[2].clone())
    }

    fn red() -> ColorRef {
        ColorRef::Direct { rgba: Rgba([1.0, 0.0, 0.0, 1.0]) }
    }

    fn req(cut: &Id, layer: &Id, shape: FigureShape, mode: FigureMode) -> FigureRequest {
        FigureRequest { cut: cut.clone(), frame: 0, layer: layer.clone(), shape, mode, width: 4.0, color: red(), fill_color: None, tip: VectorTip::Marker, opacity: 1.0, aliased: false, selection: None }
    }

    fn render(s: &CoreSession, cut: &Id) -> Image {
        aiai_render::render_frame(&s.project, &s.blobs, cut, 0, &aiai_render::RenderOptions { camera: false, ..Default::default() }).unwrap().image
    }

    fn cel<'a>(s: &'a CoreSession, cut: &Id, layer: &Id) -> &'a CelData {
        let cd = cut_data(&s.project, cut).unwrap();
        match &exposed_cel(&s.project, cd, layer, 0).unwrap().data {
            NodeKind::Cel(c) => c,
            _ => unreachable!(),
        }
    }

    #[test]
    fn shapes_have_the_expected_outlines() {
        // polígono: primeiro vértice para cima, n lados
        let o = FigureShape::Polygon { center: [50.0, 50.0], radius: [20.0, 20.0], sides: 6, angle: 0.0 }.outline().unwrap();
        assert_eq!(o.points.len(), 6);
        assert!((o.points[0].x - 50.0).abs() < 1e-3 && (o.points[0].y - 30.0).abs() < 1e-3 && o.closed);
        assert!(FigureShape::Polygon { center: [0.0; 2], radius: [5.0; 2], sides: 2, angle: 0.0 }.outline().is_err());
        // elipse: 4 pontos com alças, passando pelos extremos
        let e = FigureShape::Ellipse { rect: [10.0, 20.0, 80.0, 40.0], angle: 0.0 }.outline().unwrap();
        assert_eq!(e.points.len(), 4);
        let s = outline_stroke(&e, 1.0, VectorTip::Marker, &red(), 1.0, false);
        let samp = sample_stroke(&s);
        let max_x = samp.iter().map(|q| q.x).fold(f32::MIN, f32::max);
        let min_y = samp.iter().map(|q| q.y).fold(f32::MAX, f32::min);
        assert!((max_x - 90.0).abs() < 0.6 && (min_y - 20.0).abs() < 0.6, "{max_x} {min_y}");
        // ponto a 45°: na elipse (erro da cúbica < 0,1%)
        let at45 = samp.iter().map(|q| (((q.x - 50.0) / 40.0).powi(2) + ((q.y - 40.0) / 20.0).powi(2)).sqrt()).fold(0.0f32, |a, r| a.max((r - 1.0).abs()));
        assert!(at45 < 0.01, "desvio da elipse {at45}");
        // retângulo arredondado: dentro da caixa, cantos cortados
        let r = FigureShape::Rect { rect: [0.0, 0.0, 100.0, 50.0], radius: 10.0, angle: 0.0 }.outline().unwrap();
        assert!(r.points.iter().all(|q| q.x >= -1e-3 && q.x <= 100.001 && q.y >= -1e-3 && q.y <= 50.001));
        assert!(!r.points.iter().any(|q| q.x.abs() < 1e-3 && q.y.abs() < 1e-3), "o canto não é mais um vértice");
        // retângulo de caixa invertida (arrasto para cima/esquerda) = o mesmo
        let a = FigureShape::Rect { rect: [100.0, 50.0, -100.0, -50.0], radius: 0.0, angle: 0.0 }.outline().unwrap();
        assert_eq!(a.points.len(), 4);
        assert_eq!((a.points[0].x, a.points[0].y), (0.0, 0.0));
    }

    #[test]
    fn two_step_curve_passes_through_the_pulled_point() {
        let o = FigureShape::Curve { from: [0.0, 100.0], to: [200.0, 100.0], through: [100.0, 40.0] }.outline().unwrap();
        let s = outline_stroke(&o, 1.0, VectorTip::Marker, &red(), 1.0, false);
        let samp = sample_stroke(&s);
        let near = samp.iter().map(|q| (q.x - 100.0).hypot(q.y - 40.0)).fold(f32::MAX, f32::min);
        assert!(near < 1.0, "a curva não passa pelo ponto puxado ({near})");
        assert!(o.points.iter().all(|q| q.handle.is_some()), "as alças ficam nos pontos (editáveis)");
    }

    #[test]
    fn bezier_click_is_a_corner_and_drag_is_a_handle() {
        let o = FigureShape::Bezier { anchors: vec![BezierAnchor { at: [0.0, 0.0], handle: None }, BezierAnchor { at: [100.0, 0.0], handle: Some([30.0, 30.0]) }, BezierAnchor { at: [200.0, 0.0], handle: None }], closed: false }.outline().unwrap();
        assert!(o.points[0].corner && !o.points[1].corner && o.points[2].corner);
        assert_eq!(o.points[1].handle, Some([30.0, 30.0]));
        // a curva desce depois do ponto do meio (alça para baixo-direita) e sobe antes dele
        let s = outline_stroke(&o, 1.0, VectorTip::Marker, &red(), 1.0, false);
        let samp = sample_stroke(&s);
        assert!(samp.iter().any(|q| q.x > 110.0 && q.x < 150.0 && q.y > 3.0));
        assert!(samp.iter().any(|q| q.x > 50.0 && q.x < 90.0 && q.y < -3.0));
    }

    #[test]
    fn line_and_fill_on_each_layer_kind() {
        let (mut s, cut, vec_l, ras_l, fill_l) = session();
        let rect = FigureShape::Rect { rect: [20.0, 20.0, 60.0, 40.0], radius: 0.0, angle: 0.0 };
        // vetorial: traço + região de preenchimento com a cor secundária
        let mut r = req(&cut, &vec_l, rect.clone(), FigureMode::LineFill);
        r.fill_color = Some(ColorRef::Direct { rgba: Rgba([0.0, 0.0, 1.0, 1.0]) });
        let c = figure_command(&s.project, &mut s.blobs, &r).unwrap();
        s.apply(c).unwrap();
        let cd = cel(&s, &cut, &vec_l);
        assert_eq!((cd.strokes.len(), cd.fills.len()), (1, 1));
        assert!(cd.strokes[0].closed && cd.strokes[0].tip == VectorTip::Marker);
        let img = render(&s, &cut);
        let mid = img.get(50, 40);
        assert!(mid[2] > 0.9 && mid[0] < 0.1, "dentro: azul do preenchimento {mid:?}");
        let edge = img.get(20, 40);
        assert!(edge[0] > 0.9, "borda: vermelho da linha {edge:?}");
        s.undo().unwrap();
        assert_eq!(cel(&s, &cut, &vec_l).fills.len(), 0, "um passo de desfazer");
        assert!(cel(&s, &cut, &vec_l).strokes.is_empty());
        // raster: só preenchimento, pixels
        let c = figure_command(&s.project, &mut s.blobs, &req(&cut, &ras_l, FigureShape::Ellipse { rect: [100.0, 10.0, 80.0, 80.0], angle: 0.0 }, FigureMode::Fill)).unwrap();
        s.apply(c).unwrap();
        assert!(!cel(&s, &cut, &ras_l).tiles.is_empty());
        let img = render(&s, &cut);
        assert!(img.get(140, 50)[0] > 0.99 && img.get(101, 11)[3] < 0.01, "círculo cheio, canto da caixa vazio");
        // preenchimento: a linha vira região também
        let c = figure_command(&s.project, &mut s.blobs, &req(&cut, &fill_l, FigureShape::Line { from: [10.0, 100.0], to: [190.0, 100.0] }, FigureMode::Line)).unwrap();
        s.apply(c).unwrap();
        let cd = cel(&s, &cut, &fill_l);
        assert_eq!((cd.strokes.len(), cd.fills.len()), (0, 1));
        assert!(render(&s, &cut).get(100, 100)[0] > 0.9);
    }

    #[test]
    fn lasso_always_fills_and_raster_respects_the_selection() {
        let (mut s, cut, _, ras_l, _) = session();
        let mut r = req(&cut, &ras_l, FigureShape::Lasso { points: vec![[10.0, 10.0], [190.0, 10.0], [190.0, 110.0], [10.0, 110.0]] }, FigureMode::Line);
        r.selection = Some(SelectShape::Rect { rect: [0.0, 0.0, 100.0, 120.0] });
        let c = figure_command(&s.project, &mut s.blobs, &r).unwrap();
        s.apply(c).unwrap();
        let img = render(&s, &cut);
        assert!(img.get(50, 60)[0] > 0.99, "laço preenche mesmo no modo linha");
        assert!(img.get(150, 60)[3] < 0.01, "fora da seleção não pinta");
    }

    #[test]
    fn invalid_requests_are_errors() {
        let (mut s, cut, vec_l, _, _) = session();
        assert!(figure_command(&s.project, &mut s.blobs, &req(&cut, &vec_l, FigureShape::Polyline { points: vec![[1.0, 1.0]], closed: false }, FigureMode::Line)).is_err());
        let mut r = req(&cut, &vec_l, FigureShape::Line { from: [0.0, 0.0], to: [10.0, 10.0] }, FigureMode::Line);
        r.width = 0.0;
        assert!(figure_command(&s.project, &mut s.blobs, &r).is_err());
        // forma toda fora do canvas no preenchimento
        assert!(figure_command(&s.project, &mut s.blobs, &req(&cut, &vec_l, FigureShape::Rect { rect: [500.0, 500.0, 10.0, 10.0], radius: 0.0, angle: 0.0 }, FigureMode::Fill)).is_err());
    }

    #[test]
    fn lines_layer_is_redone_in_place() {
        let (mut s, cut, vec_l, ras_l, _) = session();
        let mut lr = LinesRequest { cut: cut.clone(), frame: 0, layer: vec_l.clone(), kind: LinesKind::Focus { center: [100.0, 60.0], clear: [30.0, 20.0] }, count: 40, length: [20.0, 80.0], width: [1.0, 3.0], color: red(), seed: 3 };
        let c = lines_layer_command(&s.project, &lr, false).unwrap();
        s.apply(c).unwrap();
        assert_eq!(cel(&s, &cut, &vec_l).strokes.len(), 40);
        let l = layer_data(&s.project, &vec_l).unwrap();
        assert_eq!(lines_params(l).map(|r| r.count), Some(40));
        lr.count = 25;
        lr.kind = LinesKind::Speed { angle: 30.0, area: [0.0, 0.0, 200.0, 120.0] };
        let c = lines_layer_command(&s.project, &lr, true).unwrap();
        s.apply(c).unwrap();
        assert_eq!(cel(&s, &cut, &vec_l).strokes.len(), 25, "refazer troca os traços");
        assert!(matches!(lines_params(layer_data(&s.project, &vec_l).unwrap()).unwrap().kind, LinesKind::Speed { .. }));
        lr.layer = ras_l;
        assert!(lines_layer_command(&s.project, &lr, true).is_err());
        lr.layer = vec_l;
        lr.count = 0;
        assert!(lines_layer_command(&s.project, &lr, true).is_err());
    }
}
