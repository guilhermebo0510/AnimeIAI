//! Render de um quadro de um corte: árvore de camadas (filhos de baixo para cima) → imagem.
//!
//! Ordem dentro de um cel: imagem importada → tiles raster → regiões de preenchimento → traços.
//! Camadas usadas como máscara são renderizadas mesmo invisíveis (convenção: deixe a camada-máscara oculta).

use aiai_paint::blend::blend_pixel;
use aiai_core::*;
use aiai_paint::{Affine, Image, Mask, sample_stroke, rasterize_stroke, tile, transform_image};
use rayon::prelude::*;

#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Inclui camadas de guia (régua, 3D de guia, construção).
    pub include_guides: bool,
    /// Inclui camadas de referência (mesa de luz).
    pub include_reference: bool,
    /// Se definido, renderiza só estas camadas (e as pastas que as contêm).
    pub only_layers: Option<Vec<Id>>,
    /// Achata sobre esta cor opaca (sRGB 0..1); `None` = fundo transparente.
    pub background: Option<[f32; 3]>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { include_guides: false, include_reference: false, only_layers: None, background: None }
    }
}

#[derive(Debug)]
pub struct RenderOutput {
    pub image: Image,
    /// Coisas que não foram desenhadas (tipo de camada ainda não suportado, blob ausente…). Nunca silenciar.
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("corte não encontrado: {0}")]
    NoCut(Id),
    #[error("quadro {frame} fora da duração {duration}")]
    FrameOutOfRange { frame: u32, duration: u32 },
}

pub struct Renderer<'a> {
    pub project: &'a Project,
    pub blobs: &'a BlobStore,
    width: u32,
    height: u32,
    warnings: Vec<String>,
}

/// Tamanho do canvas do corte.
pub fn cut_canvas(project: &Project, cut: &CutData) -> (u32, u32) {
    match cut.canvas {
        Some([w, h]) => (w, h),
        None => (project.settings.width, project.settings.height),
    }
}

pub fn render_frame(project: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, opts: &RenderOptions) -> Result<RenderOutput, RenderError> {
    let cut_node = project.node(cut_id).ok_or_else(|| RenderError::NoCut(cut_id.clone()))?;
    let cut = match &cut_node.data {
        NodeKind::Cut(c) => c,
        _ => return Err(RenderError::NoCut(cut_id.clone())),
    };
    if frame >= cut.duration {
        return Err(RenderError::FrameOutOfRange { frame, duration: cut.duration });
    }
    let (w, h) = cut_canvas(project, cut);
    let mut r = Renderer { project, blobs, width: w, height: h, warnings: Vec::new() };
    let allowed = opts.only_layers.as_ref().map(|ids| {
        let mut set = std::collections::BTreeSet::new();
        for id in ids {
            set.insert(id.clone());
            set.extend(project.ancestors(id));
            set.extend(project.subtree(id));
        }
        set
    });
    let mut buf = Image::new(w, h);
    r.composite_children(&mut buf, &cut_node.children, cut, frame, opts, allowed.as_ref());
    if let Some(bg) = opts.background {
        buf.data.par_iter_mut().for_each(|p| {
            let k = 1.0 - p[3];
            *p = [p[0] + bg[0] * k, p[1] + bg[1] * k, p[2] + bg[2] * k, 1.0];
        });
    }
    Ok(RenderOutput { image: buf, warnings: r.warnings })
}

impl<'a> Renderer<'a> {
    fn composite_children(
        &mut self,
        buf: &mut Image,
        children: &[Id],
        cut: &CutData,
        frame: u32,
        opts: &RenderOptions,
        allowed: Option<&std::collections::BTreeSet<Id>>,
    ) {
        // Alfa da última camada "base" (não recortada), para máscaras de recorte.
        let mut clip_base: Option<Vec<f32>> = None;
        let project = self.project;
        for id in children {
            let Some(node) = project.node(id) else { continue };
            let NodeKind::Layer(layer) = &node.data else { continue };
            if allowed.is_some_and(|a| !a.contains(id)) {
                continue;
            }
            if !layer.visible {
                if !layer.clip_to_below {
                    clip_base = None;
                }
                continue;
            }
            if (layer.kind == LayerType::Guide && !opts.include_guides) || (layer.kind == LayerType::Reference && !opts.include_reference) {
                continue;
            }
            let content: Option<Image> = match layer.kind {
                LayerType::Group => {
                    if layer.blend == BlendMode::PassThrough && layer.opacity >= 1.0 && layer.mask.is_none() && !layer.clip_to_below {
                        self.composite_children(buf, &node.children, cut, frame, opts, allowed);
                        clip_base = None;
                        continue;
                    }
                    let mut g = Image::new(self.width, self.height);
                    self.composite_children(&mut g, &node.children, cut, frame, opts, allowed);
                    Some(g)
                }
                k if k.holds_cels() => self.layer_image(id, node, layer, cut, frame),
                other => {
                    self.warnings.push(format!("camada '{}' do tipo {other:?} ainda não é renderizada", node.name));
                    None
                }
            };
            let Some(img) = content else {
                if !layer.clip_to_below {
                    clip_base = Some(vec![0.0; buf.data.len()]);
                }
                continue;
            };
            let mask = layer.mask.as_ref().and_then(|m| self.mask_values(m, cut, frame));
            let mode = if layer.blend == BlendMode::PassThrough { BlendMode::Normal } else { layer.blend };
            let clip = if layer.clip_to_below { clip_base.as_deref() } else { None };
            let opacity = layer.opacity;
            buf.data.par_iter_mut().enumerate().for_each(|(i, d)| {
                let mut a = opacity;
                if let Some(m) = &mask {
                    a *= m[i];
                }
                if let Some(c) = clip {
                    a *= c[i];
                }
                blend_pixel(d, img.data[i], mode, a);
            });
            if !layer.clip_to_below {
                clip_base = Some(img.data.iter().map(|p| p[3]).collect());
            }
        }
    }

    /// Cel exposto pela camada neste quadro: pela trilha do timesheet; sem trilha, o cel de menor número (camada estática).
    fn exposed_cel(&self, layer_id: &Id, layer_node: &Node, cut: &CutData, frame: u32) -> Option<&'a Node> {
        let project = self.project;
        match cut.xsheet.track_for_layer(layer_id) {
            Some(track) => match track.exposed_at(frame)? {
                CellRef::Cel { number } => project.cel_by_number(layer_id, number),
                CellRef::Empty => None,
            },
            None => layer_node
                .children
                .iter()
                .filter_map(|c| project.node(c))
                .filter(|n| matches!(n.data, NodeKind::Cel(_)))
                .min_by_key(|n| match &n.data {
                    NodeKind::Cel(c) => c.number,
                    _ => u32::MAX,
                }),
        }
    }

    fn layer_image(&mut self, id: &Id, node: &Node, layer: &LayerData, cut: &CutData, frame: u32) -> Option<Image> {
        let cel_node = self.exposed_cel(id, node, cut, frame)?;
        let NodeKind::Cel(cel) = &cel_node.data else { return None };
        let hide_trace = layer.params.get("hide_color_trace").and_then(|v| v.as_bool()).unwrap_or(false);
        let mut img = self.render_cel(cel, hide_trace, &node.name);
        let [tx, ty, s, rot] = cel.transform;
        let xf = Affine::from_trs(tx + layer.offset[0], ty + layer.offset[1], s, rot, [self.width as f32 / 2.0, self.height as f32 / 2.0]);
        if !xf.is_identity() {
            img = transform_image(&img, xf, self.width, self.height);
        }
        Some(img)
    }

    pub fn resolve_color(&mut self, c: &ColorRef) -> [f32; 4] {
        match c {
            ColorRef::Direct { rgba } => rgba.0,
            ColorRef::Palette { palette, color } => {
                let found = match self.project.node(palette).map(|n| &n.data) {
                    Some(NodeKind::Palette(p)) => p.resolve(color),
                    _ => None,
                };
                match found {
                    Some(c) => c.0,
                    None => {
                        self.warnings.push(format!("cor de paleta ausente ({palette}/{color}) — desenhada em magenta"));
                        [1.0, 0.0, 1.0, 1.0]
                    }
                }
            }
        }
    }

    /// Rasteriza um cel no tamanho do canvas (sem a transformação do cel).
    pub fn render_cel(&mut self, cel: &CelData, hide_color_trace: bool, layer_name: &str) -> Image {
        let (w, h) = (self.width, self.height);
        let mut img = Image::new(w, h);
        if let Some(hash) = &cel.image {
            match self.blobs.get(hash).map(|b| Image::decode(&b)) {
                Some(Ok(src)) => img.draw_over(&src, 0, 0, 1.0),
                Some(Err(e)) => self.warnings.push(format!("{layer_name}: imagem ilegível: {e}")),
                None => self.warnings.push(format!("{layer_name}: blob de imagem ausente {hash}")),
            }
        }
        for t in &cel.tiles {
            match self.blobs.get(&t.blob).map(|b| tile::decode_tile(&b)) {
                Some(Ok(px)) => tile::blit_tile(&mut img, &px, t.x, t.y, [0, 0]),
                Some(Err(e)) => self.warnings.push(format!("{layer_name}: tile ({}, {}) ilegível: {e}", t.x, t.y)),
                None => self.warnings.push(format!("{layer_name}: tile ({}, {}) sem blob", t.x, t.y)),
            }
        }
        for f in &cel.fills {
            let Some(hash) = &f.mask else { continue };
            let color = self.resolve_color(&f.color);
            match self.blobs.get(hash).map(|b| Mask::decode(&b)) {
                Some(Ok(m)) => {
                    let [bx, by, _, _] = f.bounds;
                    let premul = [color[0] * color[3], color[1] * color[3], color[2] * color[3], color[3]];
                    for my in 0..m.height {
                        let y = by + my as i32;
                        if y < 0 || y >= h as i32 {
                            continue;
                        }
                        for mx in 0..m.width {
                            let x = bx + mx as i32;
                            if x < 0 || x >= w as i32 {
                                continue;
                            }
                            let cov = m.get(mx, my);
                            if cov > 0.0 {
                                let i = img.idx(x as u32, y as u32);
                                Image::over(&mut img.data[i], premul, cov);
                            }
                        }
                    }
                }
                Some(Err(e)) => self.warnings.push(format!("{layer_name}: máscara ilegível: {e}")),
                None => self.warnings.push(format!("{layer_name}: máscara sem blob {hash}")),
            }
        }
        for s in &cel.strokes {
            if hide_color_trace && s.color_trace {
                continue;
            }
            let color = self.resolve_color(&s.color);
            let samples = sample_stroke(s);
            let Some(cov) = rasterize_stroke(&samples, s.aliased, w, h) else { continue };
            let a = color[3] * s.opacity;
            let premul = [color[0] * a, color[1] * a, color[2] * a, a];
            for yy in 0..cov.height {
                let y = cov.y0 as u32 + yy;
                let row = img.idx(cov.x0 as u32, y);
                let crow = (yy * cov.width) as usize;
                for xx in 0..cov.width as usize {
                    let c = cov.cov[crow + xx];
                    if c > 0.0 {
                        Image::over(&mut img.data[row + xx], premul, c);
                    }
                }
            }
        }
        img
    }

    /// Valores de máscara (luminância × alfa) da camada `mask_id` no quadro.
    fn mask_values(&mut self, mask_id: &Id, cut: &CutData, frame: u32) -> Option<Vec<f32>> {
        let project = self.project;
        let node = project.node(mask_id)?;
        let NodeKind::Layer(l) = &node.data else { return None };
        let img = self.layer_image(mask_id, node, l, cut, frame)?;
        Some(
            img.data
                .iter()
                .map(|p| {
                    if p[3] <= 0.0 {
                        0.0
                    } else {
                        let (r, g, b) = (p[0] / p[3], p[1] / p[3], p[2] / p[3]);
                        (0.2126 * r + 0.7152 * g + 0.0722 * b) * p[3]
                    }
                })
                .collect(),
        )
    }
}
