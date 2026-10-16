//! Edição e entrega (S18): documento de edição (sequência de cortes com transições, legendas, créditos), render de
//! qualquer quadro da edição, mixagem do áudio de todos os cortes na linha do tempo e exportação MP4.

use crate::context::*;
use aiai_core::*;
use aiai_paint::Image;
use aiai_paint::poly::fill_polygons;
use aiai_paint::text::{TextOptions, text_outline};
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    #[default]
    Cut,
    /// Do preto / para o preto.
    FadeBlack,
    FadeWhite,
    Dissolve,
    WipeLeft,
    WipeRight,
    /// Flash branco curto no ponto de corte.
    Flash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    pub kind: TransitionKind,
    /// Duração em quadros (sobreposição com o clipe anterior para dissolve/wipe).
    pub frames: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EditClip {
    pub cut: Id,
    /// Trecho do corte usado [in, out) em quadros; padrão = corte inteiro.
    #[serde(default)]
    pub in_frame: u32,
    #[serde(default)]
    pub out_frame: Option<u32>,
    /// Transição na entrada deste clipe.
    #[serde(default)]
    pub transition_in: Option<Transition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubtitleEntry {
    pub start_frame: u32,
    pub end_frame: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
pub struct EditDoc {
    pub clips: Vec<EditClip>,
    #[serde(default)]
    pub subtitles: Vec<SubtitleEntry>,
    /// Créditos finais (uma linha por item), rolando sobre preto por `credits_seconds`.
    #[serde(default)]
    pub credits: Vec<String>,
    #[serde(default = "d_credits_s")]
    pub credits_seconds: f32,
    /// Fade para o preto no fim (quadros).
    #[serde(default)]
    pub fade_out_end: u32,
}

fn d_credits_s() -> f32 {
    6.0
}

/// Posição de cada clipe na linha do tempo: (início global, duração usada, deslocamento no corte).
pub struct Timeline {
    pub items: Vec<(u32, u32, u32)>,
    pub credits_start: u32,
    pub total: u32,
}

impl EditDoc {
    pub fn timeline(&self, p: &Project, fps: f32) -> ToolResult<Timeline> {
        let mut items = Vec::new();
        let mut t = 0u32;
        for (k, c) in self.clips.iter().enumerate() {
            let cd = cut_data(p, &c.cut)?;
            let out = c.out_frame.unwrap_or(cd.duration).min(cd.duration);
            if c.in_frame >= out {
                return Err(ToolError::Invalid(format!("clipe {k}: trecho vazio")));
            }
            let dur = out - c.in_frame;
            // dissolve/wipe sobrepõem com o anterior
            let overlap = match (&c.transition_in, k) {
                (Some(tr), k) if k > 0 && matches!(tr.kind, TransitionKind::Dissolve | TransitionKind::WipeLeft | TransitionKind::WipeRight) => tr.frames.min(dur).min(t),
                _ => 0,
            };
            let start = t - overlap;
            items.push((start, dur, c.in_frame));
            t = start + dur;
        }
        let credits = if self.credits.is_empty() { 0 } else { (self.credits_seconds * fps).round() as u32 };
        Ok(Timeline { items, credits_start: t, total: t + credits })
    }
}

pub fn edit_command(p: &Project, idgen: &mut IdGen, name: &str, doc: &EditDoc, existing: Option<&Id>) -> ToolResult<Command> {
    let fps = p.settings.fps() as f32;
    doc.timeline(p, fps)?; // valida
    let data = NodeKind::Document(DocumentData { doc: DocumentType::Edit, content: serde_json::to_value(doc).map_err(|e| ToolError::Invalid(e.to_string()))? });
    Ok(match existing {
        Some(id) => Command::ReplaceData { id: id.clone(), data },
        None => Command::CreateNode { id: Some(idgen.next()), parent: None, index: None, name: name.into(), data },
    })
}

pub fn load_edit(p: &Project, id: &Id) -> ToolResult<EditDoc> {
    match &node(p, id)?.data {
        NodeKind::Document(d) if d.doc == DocumentType::Edit => serde_json::from_value(d.content.clone()).map_err(|e| ToolError::Invalid(format!("edição inválida: {e}"))),
        _ => Err(ToolError::Invalid(format!("{id} não é documento de edição"))),
    }
}

fn mix(a: &Image, b: &Image, k: f32) -> Image {
    let mut out = a.clone();
    for (o, q) in out.data.iter_mut().zip(&b.data) {
        for c in 0..4 {
            o[c] += (q[c] - o[c]) * k;
        }
    }
    out
}

fn fade_to(img: &mut Image, color: [f32; 3], k: f32) {
    for p in &mut img.data {
        for c in 0..3 {
            p[c] += (color[c] * p[3].max(1.0) - p[c]) * k;
        }
        p[3] += (1.0 - p[3]) * k;
    }
}

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

/// Quadro `f` da edição (render do(s) corte(s) com transição, legenda e créditos).
pub fn render_edit_frame(p: &Project, blobs: &BlobStore, doc: &EditDoc, f: u32, burn_subtitles: bool) -> ToolResult<Image> {
    let fps = p.settings.fps() as f32;
    let tl = doc.timeline(p, fps)?;
    let (w, h) = (p.settings.width, p.settings.height);
    let opts = RenderOptions { background: Some([0.0, 0.0, 0.0]), ..Default::default() };
    let render_clip = |k: usize, gf: u32| -> ToolResult<Image> {
        let (start, _dur, off) = tl.items[k];
        let local = gf - start + off;
        render_frame(p, blobs, &doc.clips[k].cut, local, &opts).map(|o| o.image).map_err(|e| ToolError::Render(e.to_string()))
    };
    let mut img = if f >= tl.credits_start {
        // créditos rolando
        let mut img = Image::filled(w, h, [0.0, 0.0, 0.0, 1.0]);
        let total = (tl.total - tl.credits_start).max(1) as f32;
        let t = (f - tl.credits_start) as f32 / total;
        let line_h = h as f32 * 0.075;
        let block = doc.credits.len() as f32 * line_h;
        let y0 = h as f32 + line_h - t * (h as f32 + block + line_h * 2.0);
        for (i, line) in doc.credits.iter().enumerate() {
            let y = y0 + i as f32 * line_h;
            if y > -line_h && y < h as f32 + line_h {
                draw_text(&mut img, line, [w as f32 / 2.0, y], h as f32 * 0.04, [0.95, 0.95, 0.95, 1.0], None);
            }
        }
        img
    } else {
        let active: Vec<usize> = (0..tl.items.len()).filter(|&k| f >= tl.items[k].0 && f < tl.items[k].0 + tl.items[k].1).collect();
        match active.as_slice() {
            [] => Image::filled(w, h, [0.0, 0.0, 0.0, 1.0]),
            [k] => {
                let mut im = render_clip(*k, f)?;
                if let Some(tr) = &doc.clips[*k].transition_in {
                    let into = f - tl.items[*k].0;
                    if into < tr.frames {
                        let a = into as f32 / tr.frames.max(1) as f32;
                        match tr.kind {
                            TransitionKind::FadeBlack => fade_to(&mut im, [0.0; 3], 1.0 - a),
                            TransitionKind::FadeWhite => fade_to(&mut im, [1.0; 3], 1.0 - a),
                            TransitionKind::Flash => fade_to(&mut im, [1.0; 3], (1.0 - a).powi(2)),
                            _ => {}
                        }
                    }
                }
                im
            }
            [a, b, ..] => {
                // sobreposição: b entra sobre a
                let (ia, ib) = (render_clip(*a, f)?, render_clip(*b, f)?);
                let tr = doc.clips[*b].transition_in.clone().unwrap_or(Transition { kind: TransitionKind::Dissolve, frames: 1 });
                let k = ((f - tl.items[*b].0) as f32 + 0.5) / tr.frames.max(1) as f32;
                match tr.kind {
                    TransitionKind::WipeLeft | TransitionKind::WipeRight => {
                        let mut out = ia.clone();
                        let edge = if tr.kind == TransitionKind::WipeLeft { (1.0 - k) * w as f32 } else { k * w as f32 };
                        for y in 0..h {
                            for x in 0..w {
                                let xf = x as f32 + 0.5;
                                let show_b = if tr.kind == TransitionKind::WipeLeft { xf >= edge } else { xf <= edge };
                                if show_b {
                                    let i = out.idx(x, y);
                                    out.data[i] = ib.data[i];
                                }
                            }
                        }
                        out
                    }
                    _ => mix(&ia, &ib, k.clamp(0.0, 1.0)),
                }
            }
        }
    };
    if doc.fade_out_end > 0 && f + doc.fade_out_end >= tl.credits_start && f < tl.credits_start {
        let left = tl.credits_start - f;
        fade_to(&mut img, [0.0; 3], 1.0 - left as f32 / doc.fade_out_end as f32);
    }
    if burn_subtitles {
        for s in &doc.subtitles {
            if f >= s.start_frame && f < s.end_frame {
                for (i, line) in s.text.lines().enumerate() {
                    let n = s.text.lines().count() as f32;
                    let y = h as f32 * 0.9 - (n - 1.0 - i as f32) * h as f32 * 0.055;
                    draw_text(&mut img, line, [w as f32 / 2.0, y], h as f32 * 0.045, [1.0, 1.0, 1.0, 1.0], Some(([0.05, 0.05, 0.08, 1.0], (h as f32 * 0.003).max(1.5))));
                }
            }
        }
    }
    Ok(img)
}

/// Áudio da edição inteira: mixa cada corte (aiai_tools::audio::mix_cut) e posiciona na linha do tempo.
/// Devolve estéreo intercalado f32 em `sample_rate`.
pub fn mix_edit(p: &Project, blobs: &BlobStore, doc: &EditDoc) -> ToolResult<Vec<f32>> {
    let fps = p.settings.fps();
    let sr = p.settings.audio_sample_rate as f64;
    let tl = doc.timeline(p, fps as f32)?;
    let total_samples = ((tl.total as f64 / fps) * sr).ceil() as usize;
    let mut out = vec![0.0f32; total_samples * 2];
    for (k, clip) in doc.clips.iter().enumerate() {
        let (start, dur, off) = tl.items[k];
        let cd = cut_data(p, &clip.cut)?;
        let buf = crate::audio::mix_cut(p, blobs, &clip.cut, Some(cd.duration)).map_err(|e| ToolError::Invalid(format!("áudio do corte {}: {e}", clip.cut)))?;
        if buf.ch.is_empty() {
            continue;
        }
        let (l, r) = (&buf.ch[0], buf.ch.get(1).unwrap_or(&buf.ch[0]));
        let s0 = ((off as f64 / fps) * sr) as usize;
        let s1 = (((off + dur) as f64 / fps) * sr) as usize;
        let dst0 = ((start as f64 / fps) * sr) as usize;
        for i in s0..s1.min(l.len()) {
            let d = dst0 + (i - s0);
            if d >= total_samples {
                break;
            }
            out[d * 2] += l[i];
            out[d * 2 + 1] += r[i];
        }
    }
    Ok(out)
}
