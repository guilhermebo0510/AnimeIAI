//! Edição e entrega (S18; v2 no S24 3.13–3.18, estilo Premiere): documento de edição com trilhas separadas de vídeo,
//! áudio e legenda ([`EditDoc`]), operações de montagem ([`ops`]: navalha, ripple, inserir/sobrescrever, slip, slide,
//! roll, mover com vínculo), legendas ([`captions`]: SRT/VTT, gerar do roteiro, verificação de cps), render de qualquer
//! quadro da edição (trilhas de vídeo compostas de V1 para cima, transições, filtros, opacidade, legendas queimadas,
//! créditos), mixagem do áudio de todas as trilhas e exportação MP4 (na bridge).
//!
//! O formato v1 (sequência de cortes) continua aceito e vira v2 ao abrir ou gravar ([`EditDoc::normalize`]); o render
//! e a mixagem do v1 ficam em [`legacy`] como referência da migração.

pub mod captions;
pub mod legacy;
mod model;
pub mod ops;

pub use legacy::{EditClip, SubtitleEntry};
pub use model::{CaptionStyle, ClipRack, EDIT_VERSION, EditDoc, EditItem, EditTrack, ItemSource, Timeline, TrackKind, Transition, TransitionAlign, TransitionKind};

use crate::context::*;
use aiai_core::*;
use aiai_paint::Image;
use aiai_paint::poly::fill_polygons;
use aiai_paint::text::{TextOptions, text_outline};
use aiai_render::{RenderOptions, render_frame};
use std::collections::HashMap;

pub fn edit_command(p: &Project, idgen: &mut IdGen, name: &str, doc: &EditDoc, existing: Option<&Id>) -> ToolResult<Command> {
    let mut doc = doc.clone();
    doc.normalize(p)?;
    doc.validate(p)?;
    let data = NodeKind::Document(DocumentData { doc: DocumentType::Edit, content: serde_json::to_value(&doc).map_err(|e| ToolError::Invalid(e.to_string()))? });
    Ok(match existing {
        Some(id) => Command::ReplaceData { id: id.clone(), data },
        None => Command::CreateNode { id: Some(idgen.next()), parent: None, index: None, name: name.into(), data },
    })
}

/// Lê o documento de edição (v1 vira v2 na hora; o arquivo só muda quando for regravado).
pub fn load_edit(p: &Project, id: &Id) -> ToolResult<EditDoc> {
    match &node(p, id)?.data {
        NodeKind::Document(d) if d.doc == DocumentType::Edit => {
            let mut doc: EditDoc = serde_json::from_value(d.content.clone()).map_err(|e| ToolError::Invalid(format!("edição inválida: {e}")))?;
            doc.normalize(p)?;
            Ok(doc)
        }
        _ => Err(ToolError::Invalid(format!("{id} não é documento de edição"))),
    }
}

/// Quadros da mídia de uma fonte, quando se sabe sem decodificar (corte: duração; áudio: `duration_s` do asset).
pub fn media_frames(p: &Project, src: &ItemSource) -> Option<u32> {
    let fps = p.settings.fps();
    match src {
        ItemSource::Cut { cut } | ItemSource::CutAudio { cut, .. } => cut_data(p, cut).ok().map(|c| c.duration),
        ItemSource::Audio { asset } => match p.node(asset).map(|n| &n.data) {
            Some(NodeKind::Asset(a)) => a.params.get("duration_s").and_then(serde_json::Value::as_f64).map(|s| (s * fps - 1e-6).ceil().max(1.0) as u32),
            _ => None,
        },
        _ => None,
    }
}

// ------------------------------------------------------------------------------------------------ imagem

pub(crate) fn mix_images(a: &Image, b: &Image, k: f32) -> Image {
    let mut out = a.clone();
    for (o, q) in out.data.iter_mut().zip(&b.data) {
        for c in 0..4 {
            o[c] += (q[c] - o[c]) * k;
        }
    }
    out
}

pub(crate) fn fade_to(img: &mut Image, color: [f32; 3], k: f32) {
    for p in &mut img.data {
        for c in 0..3 {
            p[c] += (color[c] * p[3].max(1.0) - p[c]) * k;
        }
        p[3] += (1.0 - p[3]) * k;
    }
}

/// Corpo da legenda queimada, em fração da altura do quadro.
pub const SUBTITLE_SIZE: f32 = 0.09;

/// Texto centrado (créditos, legendas) desenhado direto numa imagem.
pub fn draw_text(img: &mut Image, text: &str, center: [f32; 2], size: f32, color: [f32; 4], outline: Option<([f32; 4], f32)>) {
    if text.trim().is_empty() {
        return;
    }
    let font = crate::fx2d::default_font();
    let opt = TextOptions { size, letter_spacing: 0.0, vertical: false, center, rotation: 0.0, skew: 0.0, scale: [1.0, 1.0] };
    let Ok(o) = text_outline(font, text, &opt) else { return };
    let paint = |img: &mut Image, cov: &aiai_paint::poly::PolyCoverage, c: [f32; 4]| {
        let pm = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        for y in 0..cov.height {
            for x in 0..cov.width {
                let v = cov.cov[(y * cov.width + x) as usize];
                if v > 0.0 {
                    let i = img.idx(cov.x0 as u32 + x, cov.y0 as u32 + y);
                    Image::over(&mut img.data[i], pm, v);
                }
            }
        }
    };
    if let Some((oc, ow)) = outline {
        // contorno: o mesmo texto deslocado em 8 direções
        for k in 0..8 {
            let a = k as f32 / 8.0 * std::f32::consts::TAU;
            let shifted: Vec<Vec<[f32; 2]>> = o.contours.iter().map(|c| c.iter().map(|p| [p[0] + a.cos() * ow, p[1] + a.sin() * ow]).collect()).collect();
            if let Some(cov) = fill_polygons(&shifted, false, img.width, img.height) {
                paint(img, &cov, oc);
            }
        }
    }
    if let Some(cov) = fill_polygons(&o.contours, false, img.width, img.height) {
        paint(img, &cov, color);
    }
}

/// Largura (px) do texto numa linha com o corpo dado.
fn text_width(text: &str, size: f32) -> f32 {
    let opt = TextOptions { size, letter_spacing: 0.0, vertical: false, center: [0.0, 0.0], rotation: 0.0, skew: 0.0, scale: [1.0, 1.0] };
    text_outline(crate::fx2d::default_font(), text, &opt).map(|o| o.bounds[2] - o.bounds[0]).unwrap_or(0.0)
}

/// Créditos rolando sobre preto: `into` quadros depois do início, num bloco de `len` quadros.
pub fn render_credits(credits: &[String], w: u32, h: u32, into: u32, len: u32) -> Image {
    let mut img = Image::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
    let total = len.max(1) as f32;
    let t = into as f32 / total;
    let line_h = h as f32 * 0.075;
    let block = credits.len() as f32 * line_h;
    let y0 = h as f32 + line_h - t * (h as f32 + block + line_h * 2.0);
    for (i, line) in credits.iter().enumerate() {
        let y = y0 + i as f32 * line_h;
        if y > -line_h && y < h as f32 + line_h {
            draw_text(&mut img, line, [w as f32 / 2.0, y], h as f32 * 0.04, [0.95, 0.95, 0.95, 1.0], None);
        }
    }
    img
}

/// Legenda queimada com o estilo dado (o padrão reproduz a legenda do v1).
pub fn draw_caption(img: &mut Image, text: &str, style: &CaptionStyle, opacity: f32) {
    let (w, h) = (img.width as f32, img.height as f32);
    let size = h * style.size;
    let font = crate::fx2d::default_font();
    let lines: Vec<String> = text.lines().flat_map(|l| crate::preprod::wrap_text(font, size, l, w * style.max_width)).collect();
    let n = lines.len() as f32;
    let alpha = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * opacity];
    if let Some(bg) = style.background {
        // caixa: da linha mais larga, do topo da primeira à base da última
        let widest = lines.iter().map(|l| text_width(l, size)).fold(0.0f32, f32::max);
        let pad = size * 0.25;
        let bottom = h * style.y + size * 0.35 + pad;
        let top = h * style.y - (n - 1.0) * size * style.line_spacing - size * 0.75 - pad;
        let (x0, x1) = (((w - widest) / 2.0 - pad * 2.0).max(0.0) as u32, ((w + widest) / 2.0 + pad * 2.0).min(w) as u32);
        let c = alpha(bg);
        let pm = [c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]];
        for y in (top.max(0.0) as u32)..(bottom.min(h) as u32) {
            for x in x0..x1 {
                let i = img.idx(x, y);
                Image::over(&mut img.data[i], pm, 1.0);
            }
        }
    }
    for (i, line) in lines.iter().enumerate() {
        let y = h * style.y - (n - 1.0 - i as f32) * size * style.line_spacing;
        let outline = style.outline.map(|c| (alpha(c), (h * style.outline_width).max(1.5)));
        draw_text(img, line, [w / 2.0, y], size, alpha(style.color), outline);
    }
}

/// Imagem do asset ajustada ao quadro (contida, centrada, fundo transparente).
fn asset_image(p: &Project, blobs: &BlobStore, asset: &Id, w: u32, h: u32) -> ToolResult<Image> {
    let Some(NodeKind::Asset(a)) = p.node(asset).map(|n| &n.data) else { return Err(ToolError::NotFound(format!("asset {asset}"))) };
    let hash = a.blob.as_ref().ok_or_else(|| ToolError::Invalid(format!("asset {asset} sem imagem")))?;
    let bytes = blobs.get(hash).ok_or_else(|| ToolError::NotFound(format!("blob {hash}")))?;
    let src = ::image::load_from_memory(&bytes).map_err(|e| ToolError::Invalid(format!("imagem do asset {asset}: {e}")))?.to_rgba8();
    let (iw, ih) = (src.width().max(1), src.height().max(1));
    let s = (w as f32 / iw as f32).min(h as f32 / ih as f32);
    let (nw, nh) = (((iw as f32 * s).round() as u32).max(1), ((ih as f32 * s).round() as u32).max(1));
    let scaled = if (nw, nh) == (iw, ih) { src } else { ::image::imageops::resize(&src, nw, nh, ::image::imageops::FilterType::Triangle) };
    let mut canvas = ::image::RgbaImage::new(w, h);
    ::image::imageops::overlay(&mut canvas, &scaled, (w as i64 - nw as i64) / 2, (h as i64 - nh as i64) / 2);
    Ok(Image::from_rgba8(w, h, canvas.as_raw()))
}

/// Mistura de transição: `a` sai, `b` entra, `k` 0..1. Imagens pré-multiplicadas do mesmo tamanho.
pub fn blend_transition(kind: TransitionKind, a: &Image, b: &Image, k: f32) -> Image {
    let (w, h) = (a.width, a.height);
    let pick = |show_b: &dyn Fn(f32, f32) -> bool| {
        let mut out = a.clone();
        for y in 0..h {
            for x in 0..w {
                if show_b(x as f32 + 0.5, y as f32 + 0.5) {
                    let i = out.idx(x, y);
                    out.data[i] = b.data[i];
                }
            }
        }
        out
    };
    // deslocamento: cada pixel de saída vem de `a` deslocado por `da` e/ou de `b` deslocado por `db` (px inteiros)
    let shifted = |da: Option<i32>, db: i32| {
        let mut out = Image::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let i = out.idx(x, y);
                let xb = x as i32 - db;
                if xb >= 0 && xb < w as i32 {
                    out.data[i] = b.data[b.idx(xb as u32, y)];
                } else if let Some(da) = da {
                    let xa = x as i32 - da;
                    if xa >= 0 && xa < w as i32 {
                        out.data[i] = a.data[a.idx(xa as u32, y)];
                    }
                } else {
                    out.data[i] = a.data[i];
                }
            }
        }
        out
    };
    let (wf, hf) = (w as f32, h as f32);
    match kind {
        TransitionKind::Cut => if k < 0.5 { a.clone() } else { b.clone() },
        TransitionKind::WipeLeft => pick(&|x, _| x >= (1.0 - k) * wf),
        TransitionKind::WipeRight => pick(&|x, _| x <= k * wf),
        TransitionKind::WipeUp => pick(&|_, y| y >= (1.0 - k) * hf),
        TransitionKind::WipeDown => pick(&|_, y| y <= k * hf),
        TransitionKind::Iris => {
            let r = k * (wf * wf + hf * hf).sqrt() / 2.0;
            pick(&|x, y| (x - wf / 2.0).powi(2) + (y - hf / 2.0).powi(2) <= r * r)
        }
        TransitionKind::DipBlack | TransitionKind::DipWhite => {
            let c = if kind == TransitionKind::DipBlack { [0.0; 3] } else { [1.0; 3] };
            if k < 0.5 {
                let mut o = a.clone();
                fade_to(&mut o, c, (k * 2.0).min(1.0));
                o
            } else {
                let mut o = b.clone();
                fade_to(&mut o, c, ((1.0 - k) * 2.0).clamp(0.0, 1.0));
                o
            }
        }
        TransitionKind::SlideLeft => shifted(None, ((1.0 - k) * wf).round() as i32),
        TransitionKind::SlideRight => shifted(None, -((1.0 - k) * wf).round() as i32),
        TransitionKind::PushLeft => {
            let d = ((1.0 - k) * wf).round() as i32;
            shifted(Some(d - w as i32), d)
        }
        TransitionKind::PushRight => {
            let d = -((1.0 - k) * wf).round() as i32;
            shifted(Some(d + w as i32), d)
        }
        // dissolver (e o que sobrar: fades usados entre dois vizinhos, cruzamento de áudio numa trilha de vídeo)
        _ => mix_images(a, b, k.clamp(0.0, 1.0)),
    }
}

fn fade_color(kind: TransitionKind) -> [f32; 3] {
    if kind == TransitionKind::FadeBlack { [0.0; 3] } else { [1.0; 3] }
}

/// Fade do v1 (do preto/branco, flash) aplicado a `a` (0..1 do trecho).
fn apply_fade_in(img: &mut Image, kind: TransitionKind, a: f32) {
    match kind {
        TransitionKind::FadeBlack | TransitionKind::FadeWhite => fade_to(img, fade_color(kind), 1.0 - a),
        TransitionKind::Flash => fade_to(img, [1.0; 3], (1.0 - a).powi(2)),
        _ => {}
    }
}

struct FrameCtx<'a> {
    p: &'a Project,
    blobs: &'a BlobStore,
    doc: &'a EditDoc,
    w: u32,
    h: u32,
    opts: RenderOptions,
    images: HashMap<Id, Image>,
}

impl FrameCtx<'_> {
    /// Imagem de um item no quadro `f` da edição (alças congeladas no primeiro/último quadro da mídia), com filtros e
    /// opacidade já aplicados.
    fn item_image(&mut self, it: &EditItem, f: i64) -> ToolResult<Image> {
        let local = f - it.start as i64;
        let mut img = match &it.source {
            ItemSource::Cut { cut } => {
                let dur = cut_data(self.p, cut)?.duration;
                let sf = it.src_frame(f).clamp(0, dur.saturating_sub(1) as i64) as u32;
                render_frame(self.p, self.blobs, cut, sf, &self.opts).map(|o| o.image).map_err(|e| ToolError::Render(e.to_string()))?
            }
            ItemSource::Image { asset } => {
                if !self.images.contains_key(asset) {
                    let im = asset_image(self.p, self.blobs, asset, self.w, self.h)?;
                    self.images.insert(asset.clone(), im);
                }
                self.images[asset].clone()
            }
            ItemSource::Color { rgba } => Image::filled(self.w, self.h, *rgba),
            _ => Image::new(self.w, self.h),
        };
        if !it.filters.is_empty() {
            let ctx = aiai_fx::FxContext { frame: local.max(0) as u32, below: None, blobs: self.blobs, seed: it.id as u64 };
            // filtro inválido não derruba o quadro: o efeito é pulado (o motor do satsuei faz igual)
            let _ = aiai_fx::apply_stack(&mut img, &it.filters, &ctx);
        }
        let op = it.keyed("opacity", it.opacity, local.max(0) as f32).clamp(0.0, 1.0);
        if op < 1.0 {
            for px in &mut img.data {
                for c in px.iter_mut() {
                    *c *= op;
                }
            }
        }
        Ok(img)
    }

    /// Imagem de uma trilha de vídeo no quadro (`None` = nada nela).
    fn track_image(&mut self, track: u32, f: u32) -> ToolResult<Option<Image>> {
        let doc = self.doc;
        let items: Vec<&EditItem> = doc.items_on(track).into_iter().filter(|i| i.enabled).collect();
        // transições entre vizinhos (alças das duas mídias)
        for (k, b) in items.iter().enumerate() {
            let Some(tr) = b.transition_in.as_ref().filter(|t| t.kind != TransitionKind::Cut && !t.kind.is_fade_in() && t.frames > 0) else { continue };
            let Some(a) = k.checked_sub(1).map(|j| items[j]).filter(|a| a.end() == b.start) else { continue };
            let (l, r) = tr.split();
            let s = b.start.saturating_sub(l);
            if f >= s && f < b.start + r {
                let ia = self.item_image(a, f as i64)?;
                let ib = self.item_image(b, f as i64)?;
                let kk = ((f - s) as f32 + 0.5) / tr.frames.max(1) as f32;
                return Ok(Some(blend_transition(tr.kind, &ia, &ib, kk)));
            }
        }
        let Some(pos) = items.iter().position(|i| f >= i.start && f < i.end()) else { return Ok(None) };
        let it = items[pos];
        let mut img = self.item_image(it, f as i64)?;
        let into = f - it.start;
        if let Some(tr) = it.transition_in.as_ref().filter(|t| t.kind != TransitionKind::Cut && t.frames > 0) {
            if tr.kind.is_fade_in() {
                if into < tr.frames {
                    apply_fade_in(&mut img, tr.kind, into as f32 / tr.frames.max(1) as f32);
                }
            } else if !(pos > 0 && items[pos - 1].end() == it.start) && into < tr.frames {
                // sem vizinho: entra do nada (transparente)
                let none = Image::new(self.w, self.h);
                img = blend_transition(tr.kind, &none, &img, (into as f32 + 0.5) / tr.frames as f32);
            }
        }
        let next_two_sided = items.get(pos + 1).is_some_and(|n| n.start == it.end() && n.transition_in.as_ref().is_some_and(|t| t.kind != TransitionKind::Cut && !t.kind.is_fade_in() && t.frames > 0));
        if let Some(tr) = it.transition_out.as_ref().filter(|t| t.kind != TransitionKind::Cut && t.frames > 0 && !next_two_sided) {
            let left = it.end() - f; // 1..=dur
            if left <= tr.frames {
                let k = (tr.frames - left) as f32 + 0.5;
                let k = k / tr.frames as f32;
                if tr.kind.is_fade_in() {
                    fade_to(&mut img, fade_color(tr.kind), if tr.kind == TransitionKind::Flash { k * k } else { k });
                } else {
                    let none = Image::new(self.w, self.h);
                    img = blend_transition(tr.kind, &img, &none, k);
                }
            }
        }
        Ok(Some(img))
    }
}

/// Quadro `f` da edição: trilhas de vídeo compostas (V1 embaixo), transições, filtros, fade final, legendas e créditos.
pub fn render_edit_frame(p: &Project, blobs: &BlobStore, doc: &EditDoc, f: u32, burn_subtitles: bool) -> ToolResult<Image> {
    let fps = p.settings.fps() as f32;
    let tl = doc.timeline(p, fps)?;
    let (w, h) = (p.settings.width, p.settings.height);
    let mut img = if f >= tl.credits_start {
        render_credits(&doc.credits, w, h, f - tl.credits_start, tl.total - tl.credits_start)
    } else {
        let mut cx = FrameCtx { p, blobs, doc, w, h, opts: RenderOptions { background: Some([0.0, 0.0, 0.0]), ..Default::default() }, images: HashMap::new() };
        let mut base: Option<Image> = None;
        for t in doc.tracks_of(TrackKind::Video) {
            if t.hidden {
                continue;
            }
            let Some(layer) = cx.track_image(t.id, f)? else { continue };
            match &mut base {
                // a primeira camada entra como está (com o preto por baixo no fim): igual ao v1, bit a bit
                None => base = Some(layer),
                Some(b) => {
                    for (d, s) in b.data.iter_mut().zip(&layer.data) {
                        Image::over(d, *s, 1.0);
                    }
                }
            }
        }
        let mut img = base.unwrap_or_else(|| Image::filled(w, h, [0.0, 0.0, 0.0, 1.0]));
        // fundo preto sob o que ficou transparente (pré-multiplicado: só o alfa sobe)
        for px in &mut img.data {
            if px[3] < 1.0 {
                px[3] = 1.0;
            }
        }
        img
    };
    if doc.fade_out_end > 0 && f + doc.fade_out_end >= tl.credits_start && f < tl.credits_start {
        let left = tl.credits_start - f;
        fade_to(&mut img, [0.0; 3], 1.0 - left as f32 / doc.fade_out_end as f32);
    }
    if burn_subtitles {
        for t in doc.tracks_of(TrackKind::Subtitle) {
            if t.hidden || !t.burn {
                continue;
            }
            let track_style = t.style.clone().unwrap_or_default();
            for it in doc.items_on(t.id) {
                if !it.enabled || f < it.start || f >= it.end() {
                    continue;
                }
                if let ItemSource::Caption { text, style, .. } = &it.source {
                    let op = it.keyed("opacity", it.opacity, (f - it.start) as f32).clamp(0.0, 1.0);
                    draw_caption(&mut img, text, style.as_ref().unwrap_or(&track_style), op);
                }
            }
        }
    }
    Ok(img)
}

// ------------------------------------------------------------------------------------------------ áudio

/// Som estéreo de uma fonte de áudio na taxa `sr` (cache por fonte).
fn source_audio(p: &Project, blobs: &BlobStore, src: &ItemSource, sr: u32) -> ToolResult<(Vec<f32>, Vec<f32>)> {
    let buf = match src {
        ItemSource::CutAudio { cut, track } => {
            let cd = cut_data(p, cut)?;
            let r = match track {
                None => crate::audio::mix_cut(p, blobs, cut, Some(cd.duration)),
                Some(t) => {
                    let settings = aiai_audio::mixer::MixSettings { rate: sr, ..Default::default() };
                    let samples = (cd.duration as f64 / p.settings.fps() * sr as f64).round() as usize;
                    let want = crate::audio::mix_track(*t);
                    crate::audio::cut_clips(p, blobs, cut).map(|c| aiai_audio::mixer::render(&c.into_iter().filter(|c| c.track == want).collect::<Vec<_>>(), samples, &settings))
                }
            };
            r.map_err(|e| ToolError::Invalid(format!("áudio do corte {cut}: {e}")))?
        }
        ItemSource::Audio { asset } => aiai_audio::resample::resample(&crate::audio::asset_audio(p, blobs, asset)?, sr),
        _ => return Ok((Vec::new(), Vec::new())),
    };
    if buf.ch.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let l = buf.ch[0].clone();
    let r = buf.ch.get(1).cloned().unwrap_or_else(|| l.clone());
    Ok((l, r))
}

/// Ganho de entrada/saída de potência constante (0..1 do trecho).
fn eq_power(k: f64, fade_in: bool) -> f32 {
    let k = k.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2;
    (if fade_in { k.sin() } else { k.cos() }) as f32
}

/// Áudio da edição inteira: cada trilha de áudio (mudo/solo, volume), cada item (trecho, velocidade, volume, balanço,
/// chaves, rack, cruzamentos nas transições). Devolve estéreo intercalado f32 em `sample_rate` do projeto.
pub fn mix_edit(p: &Project, blobs: &BlobStore, doc: &EditDoc) -> ToolResult<Vec<f32>> {
    let fps = p.settings.fps();
    let sr_u = p.settings.audio_sample_rate;
    let sr = sr_u as f64;
    let tl = doc.timeline(p, fps as f32)?;
    let total_samples = ((tl.total as f64 / fps) * sr).ceil() as usize;
    let mut out = vec![0.0f32; total_samples * 2];
    let tracks = doc.tracks_of(TrackKind::Audio);
    let any_solo = tracks.iter().any(|t| t.solo);
    let mut cache: HashMap<String, std::sync::Arc<(Vec<f32>, Vec<f32>)>> = HashMap::new();
    let two_sided = |t: &Option<Transition>| t.as_ref().filter(|t| t.kind != TransitionKind::Cut && t.frames > 0).cloned();
    for t in tracks {
        if t.muted || (any_solo && !t.solo) {
            continue;
        }
        let items: Vec<&EditItem> = doc.items_on(t.id).into_iter().filter(|i| i.enabled).collect();
        for (k, it) in items.iter().enumerate() {
            let key = serde_json::to_string(&it.source).unwrap_or_default();
            let src = match cache.get(&key) {
                Some(x) => x.clone(),
                None => {
                    let x = std::sync::Arc::new(source_audio(p, blobs, &it.source, sr_u)?);
                    cache.insert(key, x.clone());
                    x
                }
            };
            let (l, r) = (&src.0, &src.1);
            if l.is_empty() {
                continue;
            }
            // trecho tocado (quadros da edição) e rampas: transição com o vizinho usa a alça da mídia
            let prev = k.checked_sub(1).map(|j| items[j]).filter(|a| a.end() == it.start);
            let next = items.get(k + 1).copied().filter(|b| b.start == it.end());
            let (mut s, mut e) = (it.start as f64, it.end() as f64);
            let mut ramp_in: Option<(f64, f64)> = None;
            let mut ramp_out: Option<(f64, f64)> = None;
            if let Some(tr) = two_sided(&it.transition_in) {
                if prev.is_some() && !tr.kind.is_fade_in() {
                    let (lf, rf) = tr.split();
                    s = (it.start as f64 - lf as f64).max(0.0);
                    ramp_in = Some((s, it.start as f64 + rf as f64));
                } else {
                    ramp_in = Some((it.start as f64, it.start as f64 + tr.frames.min(it.dur) as f64));
                }
            }
            match next.and_then(|b| two_sided(&b.transition_in).filter(|t| !t.kind.is_fade_in())) {
                Some(tr) => {
                    let (lf, rf) = tr.split();
                    e = it.end() as f64 + rf as f64;
                    ramp_out = Some((it.end() as f64 - lf as f64, e));
                }
                None => {
                    if let Some(tr) = two_sided(&it.transition_out) {
                        ramp_out = Some((it.end() as f64 - tr.frames.min(it.dur) as f64, it.end() as f64));
                    }
                }
            }
            let simple = it.speed == 1.0 && it.audio_fx.is_empty() && it.keys.is_empty() && it.volume == 1.0 && it.pan == 0.0 && t.volume == 1.0 && ramp_in.is_none() && ramp_out.is_none();
            if simple {
                // o mesmo cálculo de amostras do v1 (a migração soa igual, amostra a amostra)
                let s0 = ((it.src_in as f64 / fps) * sr) as usize;
                let s1 = (((it.src_in + it.dur) as f64 / fps) * sr) as usize;
                let dst0 = ((it.start as f64 / fps) * sr) as usize;
                for i in s0..s1.min(l.len()) {
                    let d = dst0 + (i - s0);
                    if d >= total_samples {
                        break;
                    }
                    out[d * 2] += l[i];
                    out[d * 2 + 1] += r[i];
                }
                continue;
            }
            // caminho geral: lê a mídia com a velocidade (interpolação linear; fora da mídia = silêncio)
            let d0 = ((s / fps) * sr) as usize;
            let d1 = (((e / fps) * sr) as usize).min(total_samples);
            if d1 <= d0 {
                continue;
            }
            let n = d1 - d0;
            let (mut sl, mut sr_) = (vec![0.0f32; n], vec![0.0f32; n]);
            let read = |ch: &[f32], pos: f64| -> f32 {
                if pos < 0.0 {
                    return 0.0;
                }
                let i = pos.floor() as usize;
                let fr = (pos - i as f64) as f32;
                match (ch.get(i), ch.get(i + 1)) {
                    (Some(a), Some(b)) => a + (b - a) * fr,
                    (Some(a), None) => *a,
                    _ => 0.0,
                }
            };
            for j in 0..n {
                let t_s = (d0 + j) as f64 / sr; // segundos da edição
                let pos = (it.src_in as f64 / fps + (t_s - it.start as f64 / fps) * it.speed as f64) * sr;
                sl[j] = read(l, pos);
                sr_[j] = read(r, pos);
            }
            if !it.audio_fx.is_empty() {
                let mut rack = it.audio_fx.0.clone();
                let mut b = rack.render(&aiai_audio::AudioBuf::stereo(sr_u, sl, sr_));
                b.resize(n);
                sl = b.ch[0].clone();
                sr_ = b.ch.get(1).cloned().unwrap_or_else(|| sl.clone());
            }
            for j in 0..n {
                let d = d0 + j;
                let tf = d as f64 / sr * fps; // quadro da edição (fracionário)
                let local = (tf - it.start as f64) as f32;
                let mut g = it.keyed("volume", it.volume, local.max(0.0)) * t.volume;
                if let Some((a, b)) = ramp_in {
                    if tf < b {
                        g *= eq_power((tf - a) / (b - a).max(1e-9), true);
                    }
                }
                if let Some((a, b)) = ramp_out {
                    if tf >= a {
                        g *= eq_power((tf - a) / (b - a).max(1e-9), false);
                    }
                }
                let pan = it.keyed("pan", it.pan, local.max(0.0)).clamp(-1.0, 1.0);
                let (gl, gr) = if pan == 0.0 { (g, g) } else if pan < 0.0 { (g, g * (1.0 + pan)) } else { (g * (1.0 - pan), g) };
                out[d * 2] += sl[j] * gl;
                out[d * 2 + 1] += sr_[j] * gr;
            }
        }
    }
    Ok(out)
}

/// Legenda de entrega (.srt) de cada trilha de legenda com texto: (trilha, idioma, texto SRT).
pub fn sidecar_srts(p: &Project, doc: &EditDoc) -> Vec<(u32, String, String)> {
    let fps = p.settings.fps();
    doc.tracks_of(TrackKind::Subtitle)
        .into_iter()
        .filter(|t| !t.hidden)
        .filter_map(|t| {
            let subs = captions::track_subtitles(doc, t.id, fps);
            (!subs.is_empty()).then(|| (t.id, t.lang.clone(), aiai_media::write_srt(&subs)))
        })
        .collect()
}
