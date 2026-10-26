//! Balde do S25 (à moda do Clip Studio) além do clique de tinta-e-pinta (`fill`):
//! - `editing` ("só a camada em edição" / "referir outras camadas" num clique): as cores da própria camada (ou de todas
//!   as camadas visíveis, `refer_all`) delimitam a região do clique, com tolerância, fechar intervalo e dimensionamento
//!   de área; numa camada raster a cor entra nos pixels, numa vetorial/de preenchimento vira região de preenchimento;
//! - `enclose` ("cercar e preencher"): as regiões fechadas pelas linhas que o laço `path` cerca são pintadas;
//! - `unfilled` ("pintar área não preenchida"): as regiões ainda sem tinta que o pincel `path` (raio `radius`) toca são
//!   pintadas (as frestas que o balde deixou entre linhas).
//!
//! Coordenadas em px do cel da camada (sem o movimento da camada, como o `aiai_fill`). Um pedido = um comando.

use crate::context::*;
use crate::fill::{compute_regions, fill_covers};
use crate::paint::{cel_raster, resolve_color, tiles_for_region};
use aiai_core::*;
use aiai_paint::fill::{FillParams, boundary_from_color, label_regions};
use aiai_paint::select::Selection;
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum FillAreaMode {
    /// Clique: as cores da camada (ou de todas, `refer_all`) delimitam.
    #[default]
    Editing,
    /// Laço: as regiões fechadas que ele cerca.
    Enclose,
    /// Pincel: as regiões sem tinta que ele toca.
    Unfilled,
}

fn d_tol() -> f32 {
    10.0
}
fn d_gap() -> f32 {
    3.0
}
fn d_expand() -> u32 {
    2
}
fn d_radius() -> f32 {
    20.0
}
fn d_frac() -> f32 {
    0.95
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FillAreaRequest {
    pub cut: Id,
    #[serde(default)]
    pub frame: u32,
    /// Camada que recebe a cor (raster, vetorial ou de preenchimento).
    pub layer: Id,
    #[serde(default)]
    pub mode: FillAreaMode,
    pub color: ColorRef,
    /// editing: ponto do clique [x, y].
    #[serde(default)]
    pub point: Option<[f32; 2]>,
    /// editing: tolerância de cor 0..255 (por canal, como a varinha).
    #[serde(default = "d_tol")]
    pub tolerance: f32,
    /// editing: as cores de todas as camadas visíveis delimitam (a cor continua indo para `layer`).
    #[serde(default)]
    pub refer_all: bool,
    /// enclose: o laço; unfilled: o caminho do pincel. [[x, y], ...]
    #[serde(default)]
    pub path: Vec<[f32; 2]>,
    /// unfilled: raio do pincel (px).
    #[serde(default = "d_radius")]
    pub radius: f32,
    /// enclose: fração mínima da região dentro do laço para contar como cercada (0..1).
    #[serde(default = "d_frac")]
    pub min_inside: f32,
    /// enclose/unfilled: camadas de linha que delimitam; vazio = `line_refs` de `layer`, senão a própria `layer`.
    #[serde(default)]
    pub line_layers: Vec<Id>,
    /// Fechar intervalo (px).
    #[serde(default = "d_gap")]
    pub gap: f32,
    /// enclose/unfilled: quanto a cor avança por baixo da linha (px).
    #[serde(default = "d_expand")]
    pub expand: u32,
    /// editing: dimensionamento de área (px; negativo contrai).
    #[serde(default)]
    pub scale: f32,
    /// Só dentro desta área (seleção do app, cobertura do tamanho do canvas); não vem do JSON.
    #[serde(skip)]
    #[schemars(skip)]
    pub clip: Option<Selection>,
}

/// O comando e quantas regiões ele pinta.
pub struct FillAreaPlan {
    pub command: Command,
    pub regions: usize,
}

fn render_layers(p: &Project, blobs: &BlobStore, cut: &Id, frame: u32, only: Option<Vec<Id>>) -> ToolResult<aiai_paint::Image> {
    let opts = RenderOptions { only_layers: only, include_guides: false, include_reference: false, background: None, camera: false, layer_motion: false };
    render_frame(p, blobs, cut, frame, &opts).map(|o| o.image).map_err(|e| ToolError::Render(e.to_string()))
}

fn clip_cov(sel: &mut Selection, clip: &Option<Selection>) {
    if let Some(c) = clip.as_ref().filter(|c| c.width == sel.width && c.height == sel.height) {
        for (a, b) in sel.cov.iter_mut().zip(&c.cov) {
            *a = a.min(*b);
        }
    }
}

/// Máscara (recortada na caixa) de uma cobertura do tamanho do canvas; `None` se vazia.
fn mask_of(sel: &Selection) -> Option<([i32; 4], aiai_paint::Mask)> {
    let b = sel.bounds()?;
    let (bw, bh) = ((b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
    let mut m = aiai_paint::Mask::new(bw, bh);
    for y in 0..bh {
        for x in 0..bw {
            let c = sel.cov[((b[1] as u32 + y) * sel.width + b[0] as u32 + x) as usize];
            m.data[(y * bw + x) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    Some(([b[0], b[1], bw as i32, bh as i32], m))
}

pub fn fill_area(p: &Project, blobs: &mut BlobStore, req: &FillAreaRequest) -> ToolResult<FillAreaPlan> {
    let cut = cut_data(p, &req.cut)?;
    let layer = layer_data(p, &req.layer)?;
    if !matches!(layer.kind, LayerType::Raster | LayerType::Vector | LayerType::Fill) {
        return Err(ToolError::Invalid("o balde pinta em camada raster, vetorial ou de preenchimento".into()));
    }
    if layer.locked {
        return Err(ToolError::Invalid("camada travada".into()));
    }
    let cel_node = exposed_cel(p, cut, &req.layer, req.frame).ok_or_else(|| ToolError::Invalid(format!("a camada não expõe cel no quadro {}", req.frame)))?;
    let NodeKind::Cel(cel) = &cel_node.data else { unreachable!() };
    let color = resolve_color(p, &req.color)?;
    let (w, h) = canvas_size(p, cut);
    match req.mode {
        FillAreaMode::Editing => {
            let pt = req.point.ok_or_else(|| ToolError::Invalid("editing pede `point`".into()))?;
            if !(pt[0] >= 0.0 && pt[1] >= 0.0 && pt[0] < w as f32 && pt[1] < h as f32) {
                return Err(ToolError::Invalid(format!("ponto ({}, {}) fora do canvas {w}x{h}", pt[0], pt[1])));
            }
            let reference = render_layers(p, blobs, &req.cut, req.frame, if req.refer_all { None } else { Some(vec![req.layer.clone()]) })?;
            let seed = (pt[0] as u32, pt[1] as u32);
            let boundary = boundary_from_color(&reference, seed, req.tolerance.clamp(0.0, 255.0) / 255.0 + 1e-5);
            let regions = label_regions(&boundary, w as usize, h as usize, &FillParams { gap: req.gap.max(0.0), expand: 0, line_threshold: 0.5 });
            let label = regions.label_near(seed.0, seed.1);
            if label == 0 {
                return Err(ToolError::Invalid("o clique caiu numa borda sem região por perto".into()));
            }
            let mut sel = Selection::none(w, h);
            for (c, &l) in sel.cov.iter_mut().zip(&regions.labels) {
                if l == label {
                    *c = 1.0;
                }
            }
            if req.scale != 0.0 {
                sel.grow(req.scale);
            }
            clip_cov(&mut sel, &req.clip);
            let (bounds, mask) = mask_of(&sel).ok_or_else(|| ToolError::Invalid("a região ficou vazia (dimensionamento ou seleção)".into()))?;
            if layer.kind == LayerType::Raster {
                // pixels: a cor por cima, com a cobertura da máscara
                let mut img = cel_raster(blobs, cel, w, h)?;
                let premul = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
                for y in 0..bounds[3] {
                    for x in 0..bounds[2] {
                        let c = mask.data[(y * bounds[2] + x) as usize] as f32 / 255.0;
                        if c > 0.0 {
                            let i = img.idx((bounds[0] + x) as u32, (bounds[1] + y) as u32);
                            aiai_paint::Image::over(&mut img.data[i], premul, c);
                        }
                    }
                }
                let b = [bounds[0], bounds[1], bounds[0] + bounds[2], bounds[1] + bounds[3]];
                return Ok(FillAreaPlan { command: Command::SetTiles { cel: cel_node.id.clone(), tiles: tiles_for_region(blobs, &img, b) }, regions: 1 });
            }
            let mut fills = cel.fills.clone();
            fills.retain(|f| !fill_covers(blobs, f, seed.0, seed.1));
            let hash = blobs.put(mask.encode());
            fills.push(FillRegion { id: Id(String::new()), seed: pt, color: req.color.clone(), mask: Some(hash), bounds, gradient: None });
            Ok(FillAreaPlan { command: Command::ReplaceFills { cel: cel_node.id.clone(), fills }, regions: 1 })
        }
        FillAreaMode::Enclose | FillAreaMode::Unfilled => {
            if req.path.is_empty() || (req.mode == FillAreaMode::Enclose && req.path.len() < 3) {
                return Err(ToolError::Invalid("enclose pede um laço `path` com 3+ pontos; unfilled, o caminho do pincel".into()));
            }
            let lines = if !req.line_layers.is_empty() {
                req.line_layers.clone()
            } else if !layer.line_refs.is_empty() {
                layer.line_refs.clone()
            } else {
                vec![req.layer.clone()]
            };
            let params = FillParams { gap: req.gap.max(0.0), expand: req.expand, line_threshold: 0.35 };
            let lr = compute_regions(p, blobs, &req.cut, req.frame, &lines, &params)?;
            let mut labels = match req.mode {
                FillAreaMode::Enclose => {
                    let lasso = Selection::from_shape(&aiai_paint::select::SelectShape::Lasso { points: req.path.clone() }, w, h, None).map_err(ToolError::Invalid)?;
                    aiai_paint::area::enclosed_labels(&lr.regions, &lasso, req.min_inside.clamp(0.05, 1.0))
                }
                _ => aiai_paint::area::touched_labels(&lr.regions, &req.path, req.radius.max(0.5)),
            };
            // só as que ainda não têm tinta (pintar área não preenchida): a camada alvo renderizada sozinha
            if req.mode == FillAreaMode::Unfilled {
                let painted = render_layers(p, blobs, &req.cut, req.frame, Some(vec![req.layer.clone()]))?;
                let n = lr.regions.count as usize + 1;
                let (mut tot, mut ink) = (vec![0usize; n], vec![0usize; n]);
                for (i, &l) in lr.regions.labels.iter().enumerate() {
                    if l != 0 {
                        tot[l as usize] += 1;
                        if painted.data[i][3] >= 0.5 {
                            ink[l as usize] += 1;
                        }
                    }
                }
                labels.retain(|&l| ink[l as usize] * 2 < tot[l as usize]);
            }
            if labels.is_empty() {
                return Err(ToolError::Invalid(match req.mode {
                    FillAreaMode::Enclose => "o laço não cerca nenhuma região fechada inteira".into(),
                    _ => "o pincel não tocou nenhuma região sem tinta".into(),
                }));
            }
            let mut fills = cel.fills.clone();
            let mut added = 0usize;
            for l in &labels {
                let mut sel = Selection::none(w, h);
                for (c, &x) in sel.cov.iter_mut().zip(&lr.regions.labels) {
                    if x == *l {
                        *c = 1.0;
                    }
                }
                clip_cov(&mut sel, &req.clip);
                let Some((bounds, mask)) = mask_of(&sel) else { continue };
                // semente: um pixel da região (o do meio da lista)
                let px: Vec<usize> = lr.regions.labels.iter().enumerate().filter(|(_, x)| **x == *l).map(|(i, _)| i).collect();
                let i = px[px.len() / 2];
                let (sx, sy) = ((i % w as usize) as u32, (i / w as usize) as u32);
                fills.retain(|f| !fill_covers(blobs, f, sx, sy));
                let hash = blobs.put(mask.encode());
                fills.push(FillRegion { id: Id(String::new()), seed: [sx as f32 + 0.5, sy as f32 + 0.5], color: req.color.clone(), mask: Some(hash), bounds, gradient: None });
                added += 1;
            }
            Ok(FillAreaPlan { command: Command::ReplaceFills { cel: cel_node.id.clone(), fills }, regions: added })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{DrawRequest, draw_command};
    use serde_json::json;

    fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
        s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
    }

    /// Corte 200×120 com uma camada de linha (dois quadrados fechados lado a lado) e uma de preenchimento ligada a ela.
    fn doc() -> (CoreSession, Id, Id, Id) {
        let mut s = CoreSession::for_tests("fill_area");
        s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 200, "height": 120}}) }).unwrap();
        let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(1)));
        let fill = create(&mut s, Some(&cut), "cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
        let line = create(&mut s, Some(&cut), "linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
        create(&mut s, Some(&fill), "cor1", NodeKind::Cel(CelData::new(1)));
        create(&mut s, Some(&line), "linha1", NodeKind::Cel(CelData::new(1)));
        s.apply(Command::SetProps { id: fill.clone(), name: None, data: json!({"line_refs": [line.0]}) }).unwrap();
        let sq = |x0: f32| json!({"points": [[x0, 20.0, 1.0], [x0 + 60.0, 20.0, 1.0], [x0 + 60.0, 100.0, 1.0], [x0, 100.0, 1.0]], "width": 3.0, "geometry": "polyline", "closed": true});
        let draw: DrawRequest = serde_json::from_value(json!({"cut": cut.0, "frame": 0, "layer": line.0, "curves": [sq(20.0), sq(110.0)]})).unwrap();
        s.apply(draw_command(&s.project, &draw).unwrap()).unwrap();
        (s, cut, fill, line)
    }

    fn red() -> ColorRef {
        ColorRef::Direct { rgba: Rgba([1.0, 0.0, 0.0, 1.0]) }
    }

    fn req(cut: &Id, layer: &Id, mode: FillAreaMode, path: Vec<[f32; 2]>) -> FillAreaRequest {
        FillAreaRequest { cut: cut.clone(), frame: 0, layer: layer.clone(), mode, color: red(), point: None, tolerance: 10.0, refer_all: false, path, radius: 4.0, min_inside: 0.95, line_layers: vec![], gap: 3.0, expand: 2, scale: 0.0, clip: None }
    }

    fn alpha_at(s: &CoreSession, cut: &Id, layer: &Id, x: u32, y: u32) -> f32 {
        let img = render_layers(&s.project, &s.blobs, cut, 0, Some(vec![layer.clone()])).unwrap();
        img.get(x, y)[3]
    }

    #[test]
    fn enclose_fills_only_the_regions_inside_the_lasso() {
        let (mut s, cut, fill, _) = doc();
        // laço em volta só do quadrado da esquerda
        let lasso = vec![[10.0, 10.0], [95.0, 10.0], [95.0, 110.0], [10.0, 110.0]];
        let plan = fill_area(&s.project, &mut s.blobs, &req(&cut, &fill, FillAreaMode::Enclose, lasso)).unwrap();
        assert_eq!(plan.regions, 1);
        s.apply(plan.command).unwrap();
        assert!(alpha_at(&s, &cut, &fill, 50, 60) > 0.9, "o quadrado cercado ficou pintado");
        assert!(alpha_at(&s, &cut, &fill, 140, 60) < 0.1, "o outro não");
        assert!(alpha_at(&s, &cut, &fill, 5, 5) < 0.1, "o fundo (só em parte dentro do laço) não");
        // laço em volta dos dois
        let all = vec![[5.0, 5.0], [195.0, 5.0], [195.0, 115.0], [5.0, 115.0]];
        let plan = fill_area(&s.project, &mut s.blobs, &req(&cut, &fill, FillAreaMode::Enclose, all)).unwrap();
        assert_eq!(plan.regions, 2);
        // laço que não cerca nada inteiro: erro claro, nada muda
        assert!(fill_area(&s.project, &mut s.blobs, &req(&cut, &fill, FillAreaMode::Enclose, vec![[40.0, 40.0], [60.0, 40.0], [60.0, 60.0]])).is_err());
    }

    #[test]
    fn unfilled_paints_only_regions_without_paint() {
        let (mut s, cut, fill, _) = doc();
        // a esquerda já tem tinta (azul) pelo balde normal
        let blue = ColorRef::Direct { rgba: Rgba([0.0, 0.0, 1.0, 1.0]) };
        let f = crate::fill::FillRequest { cut: cut.clone(), frame: 0, layer: fill.clone(), seeds: vec![crate::fill::FillSeed { x: 50.0, y: 60.0, color: blue }], line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35, close_leftovers: None, skip_seeds_on_line: false, only_unpainted: false };
        let c = crate::fill::fill_command(&s.project, &mut s.blobs, &f).unwrap();
        s.apply(c).unwrap();
        // pincel passando pelos dois miolos
        let plan = fill_area(&s.project, &mut s.blobs, &req(&cut, &fill, FillAreaMode::Unfilled, vec![[50.0, 60.0], [140.0, 60.0]])).unwrap();
        // o fundo de fora também é tocado no caminho entre os quadrados (e está sem tinta): ele e o da direita
        s.apply(plan.command).unwrap();
        let img = render_layers(&s.project, &s.blobs, &cut, 0, Some(vec![fill.clone()])).unwrap();
        let px = |x: u32, y: u32| img.get(x, y);
        assert!(px(50, 60)[2] > 0.9 && px(50, 60)[0] < 0.1, "a esquerda continua azul");
        assert!(px(140, 60)[0] > 0.9, "a direita ficou vermelha");
    }

    /// Camada raster 80×60 com um contorno preto 2 px de (10,10) a (50,50) e uma fresta de 2 px no lado direito.
    fn raster_doc() -> (CoreSession, Id, Id) {
        let mut s = CoreSession::for_tests("flood");
        s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 80, "height": 60}}) }).unwrap();
        let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
        let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
        let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(1)));
        let ras = create(&mut s, Some(&cut), "r", NodeKind::Layer(LayerData::new(LayerType::Raster)));
        let cel = create(&mut s, Some(&ras), "r1", NodeKind::Cel(CelData::new(1)));
        let mut img = aiai_paint::Image::new(80, 60);
        for y in 10..51u32 {
            for x in 10..51u32 {
                let border = x < 12 || x > 48 || y < 12 || y > 48;
                let gap = x > 48 && (29..31).contains(&y);
                if border && !gap {
                    let i = img.idx(x, y);
                    img.data[i] = [0.0, 0.0, 0.0, 1.0];
                }
            }
        }
        let tiles = tiles_for_region(&mut s.blobs, &img, [0, 0, 80, 60]);
        s.apply(Command::SetTiles { cel, tiles }).unwrap();
        (s, cut, ras)
    }

    #[test]
    fn editing_floods_raster_pixels_with_gap_closing() {
        // sem fechar intervalo: vaza pela fresta e pinta o fundo transparente
        let (mut s, cut, ras) = raster_doc();
        let mut r = req(&cut, &ras, FillAreaMode::Editing, vec![]);
        r.point = Some([30.0, 30.0]);
        r.gap = 0.0;
        let plan = fill_area(&s.project, &mut s.blobs, &r).unwrap();
        s.apply(plan.command).unwrap();
        assert!(alpha_at(&s, &cut, &ras, 70, 30) > 0.9, "sem fechar intervalo vaza");
        // com fechar intervalo 4 px: só dentro
        let (mut s, cut, ras) = raster_doc();
        r.layer = ras.clone();
        r.cut = cut.clone();
        r.gap = 4.0;
        let plan = fill_area(&s.project, &mut s.blobs, &r).unwrap();
        s.apply(plan.command).unwrap();
        let out = render_layers(&s.project, &s.blobs, &cut, 0, Some(vec![ras.clone()])).unwrap();
        assert!(out.get(30, 30)[0] > 0.9 && out.get(30, 30)[3] > 0.9, "dentro ficou vermelho");
        assert!(out.get(70, 30)[3] < 0.1, "fora continua transparente");
        assert!(out.get(10, 30)[0] < 0.1 && out.get(10, 30)[3] > 0.9, "a linha continua preta");
        // dimensionamento de área contrai: o pixel junto da linha fica sem cor
        let (mut s, cut, ras) = raster_doc();
        r.layer = ras.clone();
        r.cut = cut.clone();
        r.scale = -3.0;
        let plan = fill_area(&s.project, &mut s.blobs, &r).unwrap();
        s.apply(plan.command).unwrap();
        assert!(alpha_at(&s, &cut, &ras, 13, 30) < 0.1 && alpha_at(&s, &cut, &ras, 30, 30) > 0.9);
    }
}
