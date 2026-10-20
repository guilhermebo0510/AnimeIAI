//! Formato v1 da edição (S18): uma sequência de cortes com `transition_in` e legendas em quadros globais. Continua aceito
//! na entrada (documentos antigos, `aiai_edit` com `clips`) e é convertido para o v2 ao abrir/gravar
//! ([`super::EditDoc::normalize`]). O render e a mixagem v1 ficam aqui como REFERÊNCIA da migração: os testes conferem
//! que o v2 migrado desenha e soa igual ao v1, quadro a quadro e amostra a amostra.

use super::{Transition, TransitionKind, draw_text, fade_to, mix_images, SUBTITLE_SIZE};
use crate::context::*;
use aiai_core::*;
use aiai_paint::Image;
use aiai_render::{RenderOptions, render_frame};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Clipe v1: trecho de um corte na sequência.
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

/// Legenda v1 em quadros globais da edição.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SubtitleEntry {
    pub start_frame: u32,
    pub end_frame: u32,
    pub text: String,
}

/// Documento v1 inteiro (só para a referência da migração).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct EditDocV1 {
    pub clips: Vec<EditClip>,
    pub subtitles: Vec<SubtitleEntry>,
    pub credits: Vec<String>,
    pub credits_seconds: f32,
    pub fade_out_end: u32,
}

/// Linha do tempo v1: (início global, duração usada, deslocamento no corte) de cada clipe.
pub struct TimelineV1 {
    pub items: Vec<(u32, u32, u32)>,
    pub credits_start: u32,
    pub total: u32,
}

/// Dissolve/wipe sobrepõem o clipe anterior no v1.
pub(crate) fn overlaps(kind: TransitionKind) -> bool {
    matches!(kind, TransitionKind::Dissolve | TransitionKind::WipeLeft | TransitionKind::WipeRight)
}

pub fn timeline_v1(p: &Project, clips: &[EditClip], credits: usize, credits_seconds: f32, fps: f32) -> ToolResult<TimelineV1> {
    let mut items = Vec::new();
    let mut t = 0u32;
    for (k, c) in clips.iter().enumerate() {
        let cd = cut_data(p, &c.cut)?;
        let out = c.out_frame.unwrap_or(cd.duration).min(cd.duration);
        if c.in_frame >= out {
            return Err(ToolError::Invalid(format!("clipe {k}: trecho vazio")));
        }
        let dur = out - c.in_frame;
        let overlap = match (&c.transition_in, k) {
            (Some(tr), k) if k > 0 && overlaps(tr.kind) => tr.frames.min(dur).min(t),
            _ => 0,
        };
        let start = t - overlap;
        items.push((start, dur, c.in_frame));
        t = start + dur;
    }
    let credits = if credits == 0 { 0 } else { (credits_seconds * fps).round() as u32 };
    Ok(TimelineV1 { items, credits_start: t, total: t + credits })
}

impl EditDocV1 {
    pub fn timeline(&self, p: &Project, fps: f32) -> ToolResult<TimelineV1> {
        timeline_v1(p, &self.clips, self.credits.len(), self.credits_seconds, fps)
    }
}

/// Render v1 de um quadro (o código do S18, sem mudanças): referência da migração.
pub fn render_v1_frame(p: &Project, blobs: &BlobStore, doc: &EditDocV1, f: u32, burn_subtitles: bool) -> ToolResult<Image> {
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
        super::render_credits(&doc.credits, w, h, f - tl.credits_start, tl.total - tl.credits_start)
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
                let (ia, ib) = (render_clip(*a, f)?, render_clip(*b, f)?);
                let tr = doc.clips[*b].transition_in.clone().unwrap_or(Transition { kind: TransitionKind::Dissolve, frames: 1, align: Default::default() });
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
                    _ => mix_images(&ia, &ib, k.clamp(0.0, 1.0)),
                }
            }
        }
    };
    if doc.fade_out_end > 0 && f + doc.fade_out_end >= tl.credits_start && f < tl.credits_start {
        let left = tl.credits_start - f;
        fade_to(&mut img, [0.0; 3], 1.0 - left as f32 / doc.fade_out_end as f32);
    }
    if burn_subtitles {
        let size = h as f32 * SUBTITLE_SIZE;
        let font = crate::fx2d::default_font();
        for s in &doc.subtitles {
            if f >= s.start_frame && f < s.end_frame {
                let lines: Vec<String> = s.text.lines().flat_map(|l| crate::preprod::wrap_text(font, size, l, w as f32 * 0.86)).collect();
                let n = lines.len() as f32;
                for (i, line) in lines.iter().enumerate() {
                    let y = h as f32 * 0.91 - (n - 1.0 - i as f32) * size * 1.25;
                    draw_text(&mut img, line, [w as f32 / 2.0, y], size, [1.0, 1.0, 1.0, 1.0], Some(([0.04, 0.04, 0.07, 1.0], (h as f32 * 0.0038).max(1.5))));
                }
            }
        }
    }
    Ok(img)
}

/// Mixagem v1 (o código do S18): referência da migração.
pub fn mix_v1(p: &Project, blobs: &BlobStore, doc: &EditDocV1) -> ToolResult<Vec<f32>> {
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
