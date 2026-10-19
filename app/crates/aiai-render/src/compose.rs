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
    /// Se definido, renderiza só estas camadas (e as pastas que as contêm) — mesmo as ocultas.
    pub only_layers: Option<Vec<Id>>,
    /// Achata sobre esta cor opaca (sRGB 0..1); `None` = fundo transparente.
    pub background: Option<[f32; 3]>,
    /// Aplica a câmera do corte (saída no tamanho do projeto, com multiplano). `false` = o canvas inteiro do corte,
    /// sem câmera (edição, balde, model sheets).
    pub camera: bool,
    /// Aplica deslocamento/escala/rotação animados da camada (offset, chaves). `false` = o cel cru na posição do canvas,
    /// como o balde e o lint precisam (as sementes e a máscara de cor ficam em coordenadas do cel).
    pub layer_motion: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { include_guides: false, include_reference: false, only_layers: None, background: None, camera: true, layer_motion: true }
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
    /// Canvas do corte (onde os cels são desenhados).
    width: u32,
    height: u32,
    /// Tamanho da saída (quadro da câmera ou o próprio canvas).
    out_w: u32,
    out_h: u32,
    camera: Option<CameraState>,
    layer_motion: bool,
    warnings: Vec<String>,
}

/// Câmera avaliada num quadro.
#[derive(Debug, Clone, Copy)]
pub struct CameraState {
    pub center: [f32; 2],
    /// Centro de repouso (para o multiplano: planos distantes se deslocam menos a partir daqui).
    pub rest: [f32; 2],
    pub zoom: f32,
    pub rotation: f32,
    pub shake: [f32; 2],
}

impl CameraState {
    pub fn eval(cam: &CameraData, frame: u32, canvas: (u32, u32)) -> CameraState {
        let f = frame as f32;
        let k = &cam.keys;
        let base = cam.center.unwrap_or([canvas.0 as f32 / 2.0, canvas.1 as f32 / 2.0]);
        let center = [k.eval("center_x", f).unwrap_or(base[0]), k.eval("center_y", f).unwrap_or(base[1])];
        let rest = [
            k.0.get("center_x").and_then(|v| v.first()).map(|x| x.value).unwrap_or(base[0]),
            k.0.get("center_y").and_then(|v| v.first()).map(|x| x.value).unwrap_or(base[1]),
        ];
        let amp = k.eval("shake", f).unwrap_or(0.0);
        let shake = if amp != 0.0 {
            let seed = (cam.shake_seed & 0xffff_ffff) as u32;
            [amp * aiai_paint::noise::perlin(f * 0.61, 0.37, seed), amp * aiai_paint::noise::perlin(0.53, f * 0.61, seed ^ 0x9e37)]
        } else {
            [0.0, 0.0]
        };
        CameraState { center, rest, zoom: k.eval("zoom", f).unwrap_or(cam.zoom), rotation: k.eval("rotation", f).unwrap_or(cam.rotation), shake }
    }

    /// Matriz canvas → saída para um plano de profundidade `depth` (0 = plano da câmera).
    pub fn affine(&self, depth: f32, out_w: u32, out_h: u32) -> Affine {
        let kpar = 1.0 / (1.0 + depth.max(0.0));
        let c = [
            self.rest[0] + (self.center[0] - self.rest[0]) * kpar + self.shake[0],
            self.rest[1] + (self.center[1] - self.rest[1]) * kpar + self.shake[1],
        ];
        let z = 1.0 + (self.zoom - 1.0) * kpar;
        let (sn, cs) = (-self.rotation).to_radians().sin_cos();
        let (a, b, d, e) = (z * cs, -z * sn, z * sn, z * cs);
        let (ox, oy) = (out_w as f32 / 2.0, out_h as f32 / 2.0);
        Affine([a, b, ox - (a * c[0] + b * c[1]), d, e, oy - (d * c[0] + e * c[1])])
    }
}

/// Semente estável por camada (efeitos aleatórios como grão).
fn fx_seed(id: &Id) -> u64 {
    id.0.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

fn compose_affine(m2: Affine, m1: Affine) -> Affine {
    let [a1, b1, c1, d1, e1, f1] = m1.0;
    let [a2, b2, c2, d2, e2, f2] = m2.0;
    Affine([a2 * a1 + b2 * d1, a2 * b1 + b2 * e1, a2 * c1 + b2 * f1 + c2, d2 * a1 + e2 * d1, d2 * b1 + e2 * e1, d2 * c1 + e2 * f1 + f2])
}

/// Valor de propriedade animável da camada no quadro.
fn layer_prop(layer: &LayerData, prop: &str, frame: u32, default: f32) -> f32 {
    layer.keys.eval(prop, frame as f32).unwrap_or(default)
}

/// Onde o conteúdo do cel (coordenadas cruas) aparece no canvas: transformação do cel + movimento da camada no
/// quadro (sem câmera). O editor usa o inverso para o traço cair embaixo do cursor numa camada que se move.
pub fn layer_placement(layer: &LayerData, cel_transform: [f32; 4], frame: u32, width: u32, height: u32) -> Affine {
    let [tx, ty, s, rot] = cel_transform;
    let ox = layer_prop(layer, "offset_x", frame, layer.offset[0]);
    let oy = layer_prop(layer, "offset_y", frame, layer.offset[1]);
    let ks = layer_prop(layer, "scale", frame, 1.0);
    let kr = layer_prop(layer, "rotation", frame, 0.0);
    Affine::from_trs(tx + ox, ty + oy, s * ks, rot + kr, [width as f32 / 2.0, height as f32 / 2.0])
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
    let (out_w, out_h, camera) = if opts.camera {
        (project.settings.width, project.settings.height, Some(CameraState::eval(&cut.camera, frame, (w, h))))
    } else {
        (w, h, None)
    };
    let mut r = Renderer { project, blobs, width: w, height: h, out_w, out_h, camera, layer_motion: opts.layer_motion, warnings: Vec::new() };
    let allowed = opts.only_layers.as_ref().map(|ids| {
        let mut set = std::collections::BTreeSet::new();
        for id in ids {
            set.insert(id.clone());
            set.extend(project.ancestors(id));
            set.extend(project.subtree(id));
        }
        set
    });
    let mut buf = Image::new(out_w, out_h);
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
            // camada pedida explicitamente em `only_layers` entra mesmo oculta (ex.: linhas de fechamento do balde,
            // que delimitam regiões mas não aparecem no quadro final)
            if !layer.visible && allowed.is_none() {
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
                    let mut g = Image::new(self.out_w, self.out_h);
                    self.composite_children(&mut g, &node.children, cut, frame, opts, allowed);
                    Some(g)
                }
                LayerType::Effect => {
                    // Camada de efeito: aplica params.effects ao que está composto abaixo, misturado pela opacidade/máscara.
                    match aiai_fx::stack_from_params(&layer.params) {
                        Ok(stack) => {
                            let before = buf.clone();
                            let mut processed = buf.clone();
                            let ctx = aiai_fx::FxContext { frame, below: Some(&before), blobs: self.blobs, seed: fx_seed(id) };
                            for w in aiai_fx::apply_stack(&mut processed, &stack, &ctx) {
                                self.warnings.push(format!("{}: {w}", node.name));
                            }
                            let mask = layer.mask.as_ref().and_then(|m| self.mask_values(m, cut, frame));
                            let opacity = layer_prop(layer, "opacity", frame, layer.opacity).clamp(0.0, 1.0);
                            buf.data.par_iter_mut().enumerate().for_each(|(i, d)| {
                                let k = opacity * mask.as_ref().map_or(1.0, |m| m[i]);
                                let s = processed.data[i];
                                for c in 0..4 {
                                    d[c] += (s[c] - d[c]) * k;
                                }
                            });
                        }
                        Err(e) => self.warnings.push(format!("{}: {e}", node.name)),
                    }
                    continue;
                }
                LayerType::Adjustment => {
                    // Camada de ajuste: altera o que já foi composto abaixo dela (dentro da mesma pasta).
                    match layer.params.get("adjustment").map(|v| serde_json::from_value::<aiai_paint::adjust::Adjustment>(v.clone())) {
                        Some(Ok(adj)) => {
                            let mask = layer.mask.as_ref().and_then(|m| self.mask_values(m, cut, frame));
                            let clip = if layer.clip_to_below { clip_base.clone() } else { None };
                            let m: Option<Vec<f32>> = match (mask, clip) {
                                (Some(a), Some(b)) => Some(a.iter().zip(&b).map(|(x, y)| x * y).collect()),
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            };
                            adj.apply_image(buf, layer.opacity, m.as_deref());
                        }
                        Some(Err(e)) => self.warnings.push(format!("ajuste inválido em '{}': {e}", node.name)),
                        None => self.warnings.push(format!("camada de ajuste '{}' sem params.adjustment", node.name)),
                    }
                    continue;
                }
                LayerType::Particles => self.particles_image(node, layer, cut, frame),
                k if k.holds_cels() => self.layer_image(id, node, layer, cut, frame),
                other => {
                    self.warnings.push(format!("camada '{}' do tipo {other:?} ainda não é renderizada", node.name));
                    None
                }
            };
            let content = match (content, aiai_fx::stack_from_params(&layer.params)) {
                (Some(mut img), Ok(stack)) if !stack.is_empty() => {
                    let below = buf.clone();
                    let ctx = aiai_fx::FxContext { frame, below: Some(&below), blobs: self.blobs, seed: fx_seed(id) };
                    for w in aiai_fx::apply_stack(&mut img, &stack, &ctx) {
                        self.warnings.push(format!("{}: {w}", node.name));
                    }
                    Some(img)
                }
                (c, Err(e)) => {
                    self.warnings.push(format!("{}: {e}", node.name));
                    c
                }
                (c, _) => c,
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
            let opacity = layer_prop(layer, "opacity", frame, layer.opacity).clamp(0.0, 1.0);
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
        let img = self.render_cel(cel, hide_trace, &node.name);
        Some(self.place(img, layer, frame, cel.transform))
    }

    /// Partículas: simuladas no canvas do corte no tempo do quadro; sprites de cels são recortados pelo alfa.
    fn particles_image(&mut self, node: &Node, layer: &LayerData, _cut: &CutData, frame: u32) -> Option<Image> {
        let sys: aiai_anim::particles::ParticleSystem = match layer.params.get("particles").map(|v| serde_json::from_value(v.clone())) {
            Some(Ok(s)) => s,
            Some(Err(e)) => {
                self.warnings.push(format!("{}: params.particles inválido: {e}", node.name));
                return None;
            }
            None => {
                self.warnings.push(format!("{}: camada de partículas sem params.particles", node.name));
                return None;
            }
        };
        if let Err(e) = sys.validate() {
            self.warnings.push(format!("{}: {e}", node.name));
            return None;
        }
        let mut sprites = Vec::new();
        if let aiai_anim::particles::Sprite::Cels { numbers } = &sys.sprite {
            let project = self.project;
            for n in numbers {
                let Some(cn) = project.cel_by_number(&node.id, *n) else {
                    self.warnings.push(format!("{}: sprite cel {n} não existe", node.name));
                    continue;
                };
                let NodeKind::Cel(cel) = &cn.data else { continue };
                let full = self.render_cel(cel, false, &node.name);
                // recorta pela caixa do alfa
                let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
                for y in 0..full.height {
                    for x in 0..full.width {
                        if full.get(x, y)[3] > 0.0 {
                            x0 = x0.min(x);
                            y0 = y0.min(y);
                            x1 = x1.max(x);
                            y1 = y1.max(y);
                        }
                    }
                }
                if x0 > x1 {
                    continue;
                }
                let mut sp = Image::new(x1 - x0 + 1, y1 - y0 + 1);
                sp.draw_over(&full, -(x0 as i32), -(y0 as i32), 1.0);
                sprites.push(sp);
            }
        }
        let fps = self.project.settings.fps() as f32;
        let img = sys.render(self.width, self.height, frame as f32 / fps, fps, &sprites);
        Some(self.place(img, layer, frame, [0.0, 0.0, 1.0, 0.0]))
    }

    /// Aplica transformação do cel, chaves da camada e câmera (multiplano) a uma imagem do canvas.
    fn place(&self, img: Image, layer: &LayerData, frame: u32, cel_transform: [f32; 4]) -> Image {
        let mut xf = if self.layer_motion {
            layer_placement(layer, cel_transform, frame, self.width, self.height)
        } else {
            let [tx, ty, s, rot] = cel_transform;
            Affine::from_trs(tx, ty, s, rot, [self.width as f32 / 2.0, self.height as f32 / 2.0])
        };
        if let Some(cam) = &self.camera {
            xf = compose_affine(cam.affine(layer.depth, self.out_w, self.out_h), xf);
        }
        if xf.is_identity() && self.out_w == self.width && self.out_h == self.height {
            return img;
        }
        transform_image(&img, xf, self.out_w, self.out_h)
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
