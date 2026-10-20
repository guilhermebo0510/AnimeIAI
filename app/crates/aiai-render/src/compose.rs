//! Render de um quadro de um corte: árvore de camadas (filhos de baixo para cima) → imagem.
//!
//! Ordem dentro de um cel: imagem importada → tiles raster → regiões de preenchimento → traços.
//! Camadas usadas como máscara são renderizadas mesmo invisíveis (convenção: deixe a camada-máscara oculta).

use aiai_paint::blend::blend_pixel;
use aiai_core::*;
use aiai_paint::{Affine, Image, Mask, sample_stroke, rasterize_stroke, tile, transform_image};
use rayon::prelude::*;
use std::sync::Arc;

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
    let ks = layer_prop(layer, "scale", frame, layer.scale);
    let kr = layer_prop(layer, "rotation", frame, layer.rotation);
    let ax = layer_prop(layer, "anchor_x", frame, layer.anchor[0]);
    let ay = layer_prop(layer, "anchor_y", frame, layer.anchor[1]);
    let center = [width as f32 / 2.0, height as f32 / 2.0];
    if ax == 0.0 && ay == 0.0 {
        // âncora no centro: a mesma conta de sempre (projetos sem âncora renderizam idênticos)
        return Affine::from_trs(tx + ox, ty + oy, s * ks, rot + kr, center);
    }
    // âncora fora do centro: o cel com o pivô dele (centro) e, por cima, o movimento da camada girando/escalando em
    // volta da âncora
    let cel = Affine::from_trs(tx, ty, s, rot, center);
    let motion = Affine::from_trs(ox, oy, ks, kr, [center[0] + ax, center[1] + ay]);
    compose_affine(motion, cel)
}

/// Tamanho do canvas do corte.
pub fn cut_canvas(project: &Project, cut: &CutData) -> (u32, u32) {
    match cut.canvas {
        Some([w, h]) => (w, h),
        None => (project.settings.width, project.settings.height),
    }
}

/// Cache de quadro do canvas: uma foto do composto ABAIXO da primeira camada (de topo) que mudou no último render.
/// Enquanto o artista mexe numa camada, as de baixo não são recompostas. A assinatura de cada camada é a maior
/// revisão de nó na subárvore dela (o núcleo carimba a revisão em todo nó afetado por um comando) mais a da camada
/// de máscara; a chave global cobre corte (timesheet/câmera), quadro, opções, tamanho e paletas.
///
/// Acima da foto, cada camada de topo cujo conteúdo não depende do que está embaixo (não é efeito/ajuste/pasta
/// direta, nem tem light wrap) guarda a própria imagem pronta (efeitos e posição já aplicados), recortada à caixa do
/// que não é transparente: re-renderizar depois de uma pincelada só refaz a camada mexida e mistura as outras.
#[derive(Default)]
pub struct FrameCache {
    key: Option<String>,
    sigs: Vec<(Id, u64)>,
    snap: Option<FrameSnap>,
    layers: std::collections::HashMap<Id, LayerEntry>,
    /// Quantas camadas de topo o último render pulou por causa da foto (para medir/testar).
    pub last_skipped: usize,
    /// Quantas camadas de topo o último render misturou direto da imagem guardada (para medir/testar).
    pub last_reused: usize,
}

/// Teto da memória das imagens guardadas por camada (f32 RGBA recortado).
const LAYER_CACHE_CAP_BYTES: usize = 256 << 20;

struct LayerEntry {
    sig: u64,
    /// `None` = a camada não desenhou nada neste quadro.
    placed: Option<Arc<Placed>>,
    /// Máscara (luminância × alfa) da camada de máscara, no tamanho da saída.
    mask: Option<Arc<Vec<f32>>>,
    /// Avisos que o render da camada gerou (repetidos a cada uso, na mesma ordem).
    warnings: Vec<String>,
}

impl LayerEntry {
    fn bytes(&self) -> usize {
        self.placed.as_ref().map_or(0, |p| p.img.data.len() * 16) + self.mask.as_ref().map_or(0, |m| m.len() * 4)
    }
}

/// Conteúdo de uma camada na saída, recortado à caixa dos pixels com alfa (fora dela tudo é transparente, e misturar
/// transparente não muda nada: `blend_pixel` sai cedo).
struct Placed {
    x0: u32,
    y0: u32,
    img: Image,
}

impl Placed {
    /// Recorta `img` (tamanho da saída) à caixa dos pixels não transparentes; `None` se não sobra nada.
    fn crop(img: Image) -> Option<Placed> {
        let (w, h) = (img.width as usize, img.height as usize);
        if w == 0 || h == 0 {
            return None;
        }
        // `!(a <= 0)` também pega NaN, que a mistura cheia não pularia
        let rows: Vec<Option<(usize, usize)>> = img.data.par_chunks(w).map(|row| {
            let first = row.iter().position(|p| !(p[3] <= 0.0))?;
            let last = row.iter().rposition(|p| !(p[3] <= 0.0))?;
            Some((first, last))
        }).collect();
        let y0 = rows.iter().position(|r| r.is_some())?;
        let y1 = rows.iter().rposition(|r| r.is_some())?;
        let x0 = rows[y0..=y1].iter().flatten().map(|r| r.0).min()?;
        let x1 = rows[y0..=y1].iter().flatten().map(|r| r.1).max()?;
        let (cw, ch) = (x1 - x0 + 1, y1 - y0 + 1);
        if cw == w && ch == h {
            return Some(Placed { x0: 0, y0: 0, img });
        }
        let mut out = Image::new(cw as u32, ch as u32);
        out.data.par_chunks_mut(cw).enumerate().for_each(|(r, dst)| {
            let s = (y0 + r) * w + x0;
            dst.copy_from_slice(&img.data[s..s + cw]);
        });
        Some(Placed { x0: x0 as u32, y0: y0 as u32, img: out })
    }

    /// Alfa no pixel (x, y) da saída.
    fn alpha(&self, x: u32, y: u32) -> f32 {
        let (lx, ly) = (x.wrapping_sub(self.x0), y.wrapping_sub(self.y0));
        if lx < self.img.width && ly < self.img.height { self.img.data[(ly * self.img.width + lx) as usize][3] } else { 0.0 }
    }
}

/// Alfa da última camada "base" (não recortada) para as camadas com recorte, sem copiar a imagem inteira.
#[derive(Clone)]
enum ClipBase {
    /// A camada base não desenhou nada: tudo recortado.
    Empty,
    Layer(Arc<Placed>),
}

impl ClipBase {
    fn alpha(&self, x: u32, y: u32) -> f32 {
        match self {
            ClipBase::Empty => 0.0,
            ClipBase::Layer(p) => p.alpha(x, y),
        }
    }

    fn to_vec(&self, w: u32, h: u32) -> Vec<f32> {
        let mut v = vec![0.0; (w * h) as usize];
        if let ClipBase::Layer(p) = self {
            for ly in 0..p.img.height {
                for lx in 0..p.img.width {
                    v[((p.y0 + ly) * w + p.x0 + lx) as usize] = p.img.data[(ly * p.img.width + lx) as usize][3];
                }
            }
        }
        v
    }
}

struct FrameSnap {
    /// Índice da camada de topo ANTES da qual a foto foi tirada.
    index: usize,
    buf: Image,
    clip_base: Option<ClipBase>,
    warnings: Vec<String>,
}

/// O que o laço do topo usa para reaproveitar as imagens por camada.
struct LayerCtx<'c> {
    sigs: &'c [(Id, u64)],
    layers: &'c mut std::collections::HashMap<Id, LayerEntry>,
    reused: usize,
}

/// Mistura a camada recortada no quadro, só dentro da caixa dela (fora é transparente: misturar não mudaria nada).
/// Mesma conta, na mesma ordem, que a mistura do quadro inteiro.
fn blend_placed(buf: &mut Image, p: &Placed, mode: BlendMode, opacity: f32, mask: Option<&[f32]>, clip: Option<&ClipBase>) {
    let w = buf.width as usize;
    let pw = p.img.width as usize;
    let (x0, y0) = (p.x0 as usize, p.y0 as usize);
    buf.data.par_chunks_mut(w).skip(y0).take(p.img.height as usize).enumerate().for_each(|(ly, row)| {
        let y = y0 + ly;
        let src = &p.img.data[ly * pw..(ly + 1) * pw];
        for (lx, s) in src.iter().enumerate() {
            let x = x0 + lx;
            let mut a = opacity;
            if let Some(m) = mask {
                a *= m[y * w + x];
            }
            if let Some(c) = clip {
                a *= c.alpha(x as u32, y as u32);
            }
            blend_pixel(&mut row[x], *s, mode, a);
        }
    });
}

/// Índice logo acima da camada de topo mais alta cujo resultado depende do que está composto embaixo (efeito,
/// ajuste, pasta direta, light wrap): o melhor lugar para a foto quando nada mudou, porque dali para cima tudo sai
/// das imagens guardadas. 0 = nenhuma.
fn top_after_barrier(project: &Project, children: &[Id]) -> usize {
    let mut after = 0;
    for (i, id) in children.iter().enumerate() {
        if is_barrier(project, id) {
            after = i + 1;
        }
    }
    after
}

/// Teto da memória dos tiles decodificados (1 MiB por tile).
const TILE_CACHE_CAP_BYTES: usize = 128 << 20;

#[derive(Default)]
struct TileCache {
    map: std::collections::HashMap<BlobHash, (Arc<Vec<[f32; 4]>>, u64)>,
    tick: u64,
}

static TILE_CACHE: std::sync::OnceLock<std::sync::Mutex<TileCache>> = std::sync::OnceLock::new();

/// Pixels de um tile, decodificados uma vez por blob (o blob é endereçado pelo conteúdo: o hash basta como chave).
/// `None` = blob ausente.
fn decoded_tile(blobs: &BlobStore, hash: &BlobHash) -> Option<Result<Arc<Vec<[f32; 4]>>, String>> {
    let cache = TILE_CACHE.get_or_init(Default::default);
    if let Ok(mut c) = cache.lock() {
        c.tick += 1;
        let t = c.tick;
        if let Some((px, used)) = c.map.get_mut(hash) {
            *used = t;
            return Some(Ok(Arc::clone(px)));
        }
    }
    let bytes = blobs.get(hash)?;
    let px = match tile::decode_tile(&bytes) {
        Ok(px) => Arc::new(px),
        Err(e) => return Some(Err(e)),
    };
    if let Ok(mut c) = cache.lock() {
        c.tick += 1;
        let t = c.tick;
        c.map.insert(hash.clone(), (Arc::clone(&px), t));
        let per = tile::TILE_PIXELS * 16;
        while c.map.len() * per > TILE_CACHE_CAP_BYTES {
            let Some(old) = c.map.iter().min_by_key(|(_, (_, u))| *u).map(|(h, _)| h.clone()) else { break };
            c.map.remove(&old);
        }
    }
    Some(Ok(px))
}

/// A camada (visível) mexe no que está composto embaixo: efeito, ajuste, light wrap, ou pasta direta com uma dessas
/// dentro.
fn is_barrier(project: &Project, id: &Id) -> bool {
    let Some(node) = project.node(id) else { return false };
    let NodeKind::Layer(l) = &node.data else { return false };
    if !l.visible {
        return false;
    }
    match l.kind {
        LayerType::Effect | LayerType::Adjustment => true,
        LayerType::Group if l.blend == BlendMode::PassThrough && l.opacity >= 1.0 && l.mask.is_none() && !l.clip_to_below => {
            node.children.iter().any(|c| is_barrier(project, c))
        }
        _ => aiai_fx::stack_from_params(&l.params).map(|s| stack_reads_below(&s)).unwrap_or(false),
    }
}

/// A pilha de efeitos lê o composto de baixo (só o light wrap).
fn stack_reads_below(stack: &[aiai_fx::EffectInstance]) -> bool {
    stack.iter().any(|e| e.enabled && matches!(e.effect, aiai_fx::Effect::LightWrap { .. }))
}

fn subtree_revision(project: &Project, id: &Id) -> u64 {
    let mut r = project.node(id).map(|n| n.provenance.revision).unwrap_or(0);
    for d in project.subtree(id) {
        if let Some(n) = project.node(&d) {
            r = r.max(n.provenance.revision);
            if let NodeKind::Layer(l) = &n.data {
                if let Some(m) = &l.mask {
                    r = r.max(subtree_revision(project, m));
                }
            }
        }
    }
    r
}

pub fn render_frame(project: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, opts: &RenderOptions) -> Result<RenderOutput, RenderError> {
    render_frame_impl(project, blobs, cut_id, frame, opts, None)
}

/// Como [`render_frame`], reaproveitando a foto do composto de baixo guardada em `cache` (mesmo resultado, pixel a pixel).
pub fn render_frame_cached(project: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, opts: &RenderOptions, cache: &mut FrameCache) -> Result<RenderOutput, RenderError> {
    render_frame_impl(project, blobs, cut_id, frame, opts, Some(cache))
}

/// Rasteriza um cel (imagem, tiles, preenchimentos e traços, nesta ordem) no tamanho do canvas, sem a transformação do cel
/// nem o movimento da camada e sem a opacidade/modo dela: o "conteúdo cru" que a mesclagem de camadas compõe.
/// Devolve também os avisos do render (cor de paleta ausente, blob ilegível...).
pub fn render_cel_image(project: &Project, blobs: &BlobStore, cel: &CelData, hide_color_trace: bool, canvas: (u32, u32), layer_name: &str) -> (Image, Vec<String>) {
    let mut r = Renderer { project, blobs, width: canvas.0, height: canvas.1, out_w: canvas.0, out_h: canvas.1, camera: None, layer_motion: false, warnings: Vec::new() };
    let img = r.render_cel(cel, hide_color_trace, layer_name);
    (img, r.warnings)
}

fn render_frame_impl(project: &Project, blobs: &BlobStore, cut_id: &Id, frame: u32, opts: &RenderOptions, cache: Option<&mut FrameCache>) -> Result<RenderOutput, RenderError> {
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
    match cache.filter(|_| allowed.is_none()) {
        None => r.composite_children(&mut buf, &cut_node.children, cut, frame, opts, allowed.as_ref()),
        Some(cache) => {
            let palettes: u64 = project.nodes.values().filter(|n| matches!(n.data, NodeKind::Palette(_))).map(|n| n.provenance.revision).max().unwrap_or(0);
            let key = format!(
                "{}|{}|{}|{}{}{}{}|{}x{}>{}x{}|{}|{:?}",
                cut_id, cut_node.provenance.revision, frame, opts.include_guides as u8, opts.include_reference as u8, opts.camera as u8,
                opts.layer_motion as u8, w, h, out_w, out_h, palettes, project.settings
            );
            // quem desenha as camadas 3D (GPU/CPU) muda o pixel: entra na chave
            let key = format!("{key}|{}", crate::raster3d::layer3d_raster().backend());
            let sigs: Vec<(Id, u64)> = cut_node.children.iter().map(|c| (c.clone(), subtree_revision(project, c))).collect();
            let same_key = cache.key.as_deref() == Some(key.as_str());
            if !same_key {
                cache.layers.clear();
            }
            let first_changed = if same_key { sigs.iter().zip(&cache.sigs).position(|(a, b)| a != b).unwrap_or(sigs.len().min(cache.sigs.len())) } else { 0 };
            let first_changed = if same_key && sigs.len() == cache.sigs.len() { first_changed } else if same_key { first_changed.min(sigs.len().min(cache.sigs.len())) } else { 0 };
            // começa da foto se ela foi tirada antes de qualquer mudança
            let mut start = 0;
            let mut clip_base = None;
            if let Some(sn) = cache.snap.as_ref().filter(|sn| same_key && sn.index <= first_changed && sn.index <= sigs.len()) {
                buf = sn.buf.clone();
                clip_base = sn.clip_base.clone();
                r.warnings = sn.warnings.clone();
                start = sn.index;
            }
            // nova foto antes da primeira camada que mudou (é nela que o artista está mexendo); sem mudança e sem foto,
            // logo acima da última camada que depende do que está embaixo (de lá para cima tudo vem das imagens guardadas)
            let after_barrier = top_after_barrier(project, &cut_node.children);
            let snap_at = if first_changed > 0 && first_changed < sigs.len() {
                Some(first_changed)
            } else if let Some(sn) = cache.snap.as_ref().filter(|_| same_key) {
                Some(sn.index)
            } else {
                Some(after_barrier).filter(|i| *i > 0 && *i < sigs.len())
            };
            // começando da foto e com a nova foto no mesmo lugar: a de agora já é ela (não copia o quadro de novo)
            let keep_snap = start > 0 && snap_at == Some(start);
            let snap_at = if keep_snap { None } else { snap_at };
            let mut taken = None;
            let mut lc = LayerCtx { sigs: &sigs, layers: &mut cache.layers, reused: 0 };
            r.composite_range(&mut buf, &cut_node.children, cut, frame, opts, start, clip_base, snap_at, &mut taken, &mut lc);
            cache.last_skipped = start;
            cache.last_reused = lc.reused;
            if let Some(t) = taken {
                cache.snap = Some(t);
            } else if !keep_snap && (!same_key || snap_at.is_none()) {
                cache.snap = None;
            }
            // as camadas abaixo da foto não são recompostas: não precisam da imagem guardada
            let keep_from = cache.snap.as_ref().map_or(0, |sn| sn.index);
            let pos: std::collections::HashMap<&Id, usize> = sigs.iter().enumerate().map(|(i, (id, _))| (id, i)).collect();
            cache.layers.retain(|id, _| pos.get(id).is_some_and(|i| *i >= keep_from));
            let mut total: usize = cache.layers.values().map(|e| e.bytes()).sum();
            while total > LAYER_CACHE_CAP_BYTES {
                let Some(id) = cache.layers.iter().min_by_key(|(id, _)| pos.get(id).copied().unwrap_or(0)).map(|(id, _)| id.clone()) else { break };
                total -= cache.layers.remove(&id).map_or(0, |e| e.bytes());
            }
            cache.key = Some(key);
            cache.sigs = sigs;
        }
    }
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
        self.composite_inner(buf, children, cut, frame, opts, allowed, 0, None, None, &mut None, None);
    }

    /// Topo do corte com cache: começa em `start` (com o `clip_base` da foto), tira uma foto antes de `snap_at` e
    /// reaproveita/guarda as imagens por camada em `lc`.
    #[allow(clippy::too_many_arguments)]
    fn composite_range(
        &mut self,
        buf: &mut Image,
        children: &[Id],
        cut: &CutData,
        frame: u32,
        opts: &RenderOptions,
        start: usize,
        clip_base: Option<ClipBase>,
        snap_at: Option<usize>,
        taken: &mut Option<FrameSnap>,
        lc: &mut LayerCtx,
    ) {
        self.composite_inner(buf, children, cut, frame, opts, None, start, clip_base, snap_at, taken, Some(lc));
    }

    #[allow(clippy::too_many_arguments)]
    fn composite_inner(
        &mut self,
        buf: &mut Image,
        children: &[Id],
        cut: &CutData,
        frame: u32,
        opts: &RenderOptions,
        allowed: Option<&std::collections::BTreeSet<Id>>,
        start: usize,
        clip_base0: Option<ClipBase>,
        snap_at: Option<usize>,
        taken: &mut Option<FrameSnap>,
        mut lc: Option<&mut LayerCtx>,
    ) {
        // Alfa da última camada "base" (não recortada), para máscaras de recorte.
        let mut clip_base: Option<ClipBase> = clip_base0;
        let project = self.project;
        for (index, id) in children.iter().enumerate().skip(start) {
            if snap_at == Some(index) {
                *taken = Some(FrameSnap { index, buf: buf.clone(), clip_base: clip_base.clone(), warnings: self.warnings.clone() });
            }
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
            // camada 3D marcada como referência segue a regra das referências (padrão: fora do render final)
            if layer.kind == LayerType::Model3d && !opts.include_reference && aiai_core::model3d::is_reference(&layer.params) {
                continue;
            }
            // imagem guardada da camada (mesma revisão da subárvore, mesma chave do quadro)
            let sig = lc.as_ref().and_then(|lc| lc.sigs.get(index)).filter(|(sid, _)| sid == id).map(|(_, s)| *s);
            let hit = match (lc.as_deref_mut(), sig) {
                (Some(lc), Some(sig)) => match lc.layers.get(id).filter(|e| e.sig == sig) {
                    Some(e) => {
                        lc.reused += 1;
                        Some((e.placed.clone(), e.mask.clone(), e.warnings.clone()))
                    }
                    None => None,
                },
                _ => None,
            };
            let (placed, mask) = if let Some((placed, mask, warnings)) = hit {
                self.warnings.extend(warnings);
                (placed, mask)
            } else {
                let first_warning = self.warnings.len();
                if let Some((produced, placed)) = self.tiles_only_placed(id, node, layer, cut, frame) {
                    let mask = if produced { layer.mask.as_ref().and_then(|m| self.mask_values(m, cut, frame)).map(Arc::new) } else { None };
                    let placed = placed.map(Arc::new);
                    if let (Some(lc), Some(sig)) = (lc.as_deref_mut(), sig) {
                        let warnings = self.warnings[first_warning..].to_vec();
                        lc.layers.insert(id.clone(), LayerEntry { sig, placed: placed.clone(), mask: mask.clone(), warnings });
                    }
                    (placed, mask)
                } else {
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
                                    let clip = if layer.clip_to_below { clip_base.as_ref().map(|c| c.to_vec(buf.width, buf.height)) } else { None };
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
                        LayerType::Text => self.text_image(node, layer, frame),
                        LayerType::Model3d => self.model3d_image(node, layer, frame),
                        k if k.holds_cels() => self.layer_image(id, node, layer, cut, frame),
                        other => {
                            self.warnings.push(format!("camada '{}' do tipo {other:?} ainda não é renderizada", node.name));
                            None
                        }
                    };
                    let mut reads_below = false;
                    let content = match (content, aiai_fx::stack_from_params(&layer.params)) {
                        (Some(mut img), Ok(stack)) if !stack.is_empty() => {
                            // só o light wrap lê o composto de baixo: só ele paga a cópia do quadro (e não fica guardado)
                            reads_below = stack_reads_below(&stack);
                            let below = reads_below.then(|| buf.clone());
                            let ctx = aiai_fx::FxContext { frame, below: below.as_ref(), blobs: self.blobs, seed: fx_seed(id) };
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
                    let mask = if content.is_some() { layer.mask.as_ref().and_then(|m| self.mask_values(m, cut, frame)).map(Arc::new) } else { None };
                    // sem conteúdo ou tudo transparente dá no mesmo: nada a misturar e, como base de recorte, recorta tudo
                    let placed = content.and_then(Placed::crop).map(Arc::new);
                    if let (Some(lc), Some(sig), false) = (lc.as_deref_mut(), sig, reads_below) {
                        let warnings = self.warnings[first_warning..].to_vec();
                        lc.layers.insert(id.clone(), LayerEntry { sig, placed: placed.clone(), mask: mask.clone(), warnings });
                    }
                    (placed, mask)
                }
            };
            let Some(p) = placed else {
                if !layer.clip_to_below {
                    clip_base = Some(ClipBase::Empty);
                }
                continue;
            };
            let mode = if layer.blend == BlendMode::PassThrough { BlendMode::Normal } else { layer.blend };
            let clip = if layer.clip_to_below { clip_base.as_ref() } else { None };
            let opacity = layer_prop(layer, "opacity", frame, layer.opacity).clamp(0.0, 1.0);
            blend_placed(buf, &p, mode, opacity, mask.as_deref().map(|m| m.as_slice()), clip);
            if !layer.clip_to_below {
                clip_base = Some(ClipBase::Layer(p));
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

    /// Atalho do cel só de tiles (raster sem imagem, preenchimento nem traço), sem efeitos e sem movimento: monta só a
    /// caixa dos tiles, sem a imagem do canvas inteiro. `None` = não se aplica (vai pelo caminho geral); senão
    /// (a camada desenhou?, conteúdo), exatamente o que o caminho geral daria depois do recorte.
    fn tiles_only_placed(&mut self, id: &Id, node: &Node, layer: &LayerData, cut: &CutData, frame: u32) -> Option<(bool, Option<Placed>)> {
        if matches!(layer.kind, LayerType::Group | LayerType::Effect | LayerType::Adjustment | LayerType::Particles | LayerType::Text | LayerType::Model3d)
            || !layer.kind.holds_cels()
            || !layer.params.get("effects").is_none_or(|v| v.is_null() || v.as_array().is_some_and(|a| a.is_empty()))
        {
            return None;
        }
        let Some(cel_node) = self.exposed_cel(id, node, cut, frame) else { return Some((false, None)) };
        let NodeKind::Cel(cel) = &cel_node.data else { return Some((false, None)) };
        if cel.image.is_some() || !cel.fills.is_empty() || !cel.strokes.is_empty() {
            return None;
        }
        let xf = self.placement_affine(layer, frame, cel.transform);
        if !(xf.is_identity() && self.out_w == self.width && self.out_h == self.height) {
            return None;
        }
        let ts = TILE_SIZE as i64;
        let (w, h) = (self.width as i64, self.height as i64);
        let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        for t in &cel.tiles {
            let (tx0, ty0) = ((t.x as i64 * ts).max(0), (t.y as i64 * ts).max(0));
            let (tx1, ty1) = ((t.x as i64 * ts + ts).min(w), (t.y as i64 * ts + ts).min(h));
            if tx0 < tx1 && ty0 < ty1 {
                (x0, y0, x1, y1) = (x0.min(tx0), y0.min(ty0), x1.max(tx1), y1.max(ty1));
            }
        }
        let decoded: Vec<Option<Result<Arc<Vec<[f32; 4]>>, String>>> = cel.tiles.par_iter().map(|t| decoded_tile(self.blobs, &t.blob)).collect();
        let mut img = (x0 < x1).then(|| Image::new((x1 - x0) as u32, (y1 - y0) as u32));
        for (t, d) in cel.tiles.iter().zip(decoded) {
            match d {
                Some(Ok(px)) => {
                    if let Some(img) = img.as_mut() {
                        tile::blit_tile(img, &px, t.x, t.y, [x0 as i32, y0 as i32]);
                    }
                }
                Some(Err(e)) => self.warnings.push(format!("{}: tile ({}, {}) ilegível: {e}", node.name, t.x, t.y)),
                None => self.warnings.push(format!("{}: tile ({}, {}) sem blob", node.name, t.x, t.y)),
            }
        }
        Some((true, img.map(|img| Placed { x0: x0 as u32, y0: y0 as u32, img })))
    }

    fn layer_image(&mut self, id: &Id, node: &Node, layer: &LayerData, cut: &CutData, frame: u32) -> Option<Image> {
        let cel_node = self.exposed_cel(id, node, cut, frame)?;
        let NodeKind::Cel(cel) = &cel_node.data else { return None };
        let hide_trace = layer.params.get("hide_color_trace").and_then(|v| v.as_bool()).unwrap_or(false);
        // cel só de vetor com câmera/movimento/escala: rasteriza os traços já transformados, na saída (a linha continua
        // nítida num zoom; reamostrar o bitmap do canvas borrava a linha como uma imagem)
        let xf = self.placement_affine(layer, frame, cel.transform);
        let moved = !(xf.is_identity() && self.out_w == self.width && self.out_h == self.height);
        if moved && cel.image.is_none() && cel.tiles.is_empty() && cel.fills.is_empty() && !cel.strokes.is_empty() {
            let mut img = Image::new(self.out_w, self.out_h);
            self.draw_strokes(&mut img, cel, hide_trace, Some(xf));
            return Some(img);
        }
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

    /// Camada de texto: refaz o layout de `params.text` e pinta os glifos já no lugar final (movimento da camada e câmera).
    fn text_image(&mut self, node: &Node, layer: &LayerData, frame: u32) -> Option<Image> {
        use aiai_paint::text_layer::{TextParams, layout};
        let p = match TextParams::from_layer_params(&layer.params) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                self.warnings.push(format!("{}: {e}", node.name));
                return None;
            }
            None => {
                self.warnings.push(format!("camada de texto '{}' sem params.text", node.name));
                return None;
            }
        };
        if p.content.chars().all(|c| c.is_whitespace()) {
            return None;
        }
        let lay = layout(&p);
        let wanted = p.font.trim();
        if !wanted.is_empty() && !wanted.eq_ignore_ascii_case(lay.family_used()) {
            self.warnings.push(format!("{}: fonte '{wanted}' não encontrada; usando '{}'", node.name, lay.family_used()));
        }
        let colors = crate::text::TextColors {
            fill: self.resolve_color(&p.color),
            stroke: p.stroke.as_ref().map(|s| self.resolve_color(&s.color)).unwrap_or([0.0; 4]),
            shadow: p.shadow.as_ref().map(|s| self.resolve_color(&s.color)).unwrap_or([0.0; 4]),
        };
        let xf = self.placement_affine(layer, frame, [0.0, 0.0, 1.0, 0.0]);
        crate::text::render_text_layer(&p, &lay, &colors, xf, self.out_w, self.out_h)
    }

    /// Camada 3D: o modelo desenhado no canvas do corte pelo renderizador registrado (GPU no app, CPU no headless), com
    /// a câmera e a transformação de `params.model3d` avaliadas no quadro; depois segue como qualquer imagem do canvas.
    fn model3d_image(&mut self, node: &Node, layer: &LayerData, frame: u32) -> Option<Image> {
        let p = match aiai_core::model3d::Model3dParams::from_layer_params(&layer.params) {
            Some(Ok(p)) => p,
            Some(Err(e)) => {
                self.warnings.push(format!("{}: {e}", node.name));
                return None;
            }
            None => {
                self.warnings.push(format!("camada 3D '{}' sem params.model3d", node.name));
                return None;
            }
        };
        let state = p.eval(&layer.keys, frame as f32);
        let job = crate::raster3d::Layer3dJob { model_hash: &p.model_hash, state: &state, blobs: self.blobs, width: self.width, height: self.height };
        match crate::raster3d::render_layer3d(&job) {
            Ok(img) => Some(self.place(img, layer, frame, [0.0, 0.0, 1.0, 0.0])),
            Err(e) => {
                self.warnings.push(format!("{}: {e}", node.name));
                None
            }
        }
    }

    /// Aplica transformação do cel, chaves da camada e câmera (multiplano) a uma imagem do canvas.
    fn place(&self, img: Image, layer: &LayerData, frame: u32, cel_transform: [f32; 4]) -> Image {
        let xf = self.placement_affine(layer, frame, cel_transform);
        if xf.is_identity() && self.out_w == self.width && self.out_h == self.height {
            return img;
        }
        transform_image(&img, xf, self.out_w, self.out_h)
    }

    /// Canvas do cel → imagem de saída: transformação do cel, movimento da camada e câmera (multiplano).
    fn placement_affine(&self, layer: &LayerData, frame: u32, cel_transform: [f32; 4]) -> Affine {
        let mut xf = if self.layer_motion {
            layer_placement(layer, cel_transform, frame, self.width, self.height)
        } else {
            let [tx, ty, s, rot] = cel_transform;
            Affine::from_trs(tx, ty, s, rot, [self.width as f32 / 2.0, self.height as f32 / 2.0])
        };
        if let Some(cam) = &self.camera {
            xf = compose_affine(cam.affine(layer.depth, self.out_w, self.out_h), xf);
        }
        xf
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
        // tiles decodificados guardados pelo hash do blob: depois de uma pincelada só os tiles novos são decodificados
        let decoded: Vec<Option<Result<Arc<Vec<[f32; 4]>>, String>>> = cel.tiles.par_iter().map(|t| decoded_tile(self.blobs, &t.blob)).collect();
        for (t, d) in cel.tiles.iter().zip(decoded) {
            match d {
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
        self.draw_strokes(&mut img, cel, hide_color_trace, None);
        img
    }

    /// Traços vetoriais do cel sobre `img` (do tamanho de `img`); com `xf`, cada amostra é transformada e o raio
    /// escala com a transformação (a linha é rasterizada já no lugar final).
    fn draw_strokes(&mut self, img: &mut Image, cel: &CelData, hide_color_trace: bool, xf: Option<Affine>) {
        let (w, h) = (img.width, img.height);
        let k = xf.map(|a| (a.0[0] * a.0[4] - a.0[1] * a.0[3]).abs().sqrt()).unwrap_or(1.0);
        for s in &cel.strokes {
            if hide_color_trace && s.color_trace {
                continue;
            }
            let color = self.resolve_color(&s.color);
            let mut samples = sample_stroke(s);
            if let Some(a) = xf {
                for q in &mut samples {
                    let (x, y) = a.apply(q.x, q.y);
                    q.x = x;
                    q.y = y;
                    q.r *= k;
                }
            }
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
