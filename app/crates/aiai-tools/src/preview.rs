//! Prévia sem exportar vídeo: folha de contato (vários quadros de um corte ou da edição numa imagem só, com o número
//! do quadro e o tempo) — para o agente conferir movimento, câmera, legenda e transição olhando uma imagem.

use crate::context::*;
use crate::edit::{EditDoc, render_edit_frame};
use crate::fx2d::{default_font, heavy_font};
use crate::preprod::{draw_text, fill_rect};
use aiai_core::*;
use aiai_paint::{Affine, Image, transform_image};
use aiai_render::{RenderOptions, render_frame};

/// Limite de quadros por folha (cada um é um render inteiro).
pub const MAX_SHEET_FRAMES: usize = 48;

/// Quadros `start..=end` a cada `step` (no máximo [`MAX_SHEET_FRAMES`]).
pub fn frame_range(start: u32, end: u32, step: u32) -> ToolResult<Vec<u32>> {
    if end < start {
        return Err(ToolError::Invalid(format!("end {end} < start {start}")));
    }
    let v: Vec<u32> = (start..=end).step_by(step.max(1) as usize).collect();
    if v.len() > MAX_SHEET_FRAMES {
        return Err(ToolError::Invalid(format!("{} quadros na folha (máx. {MAX_SHEET_FRAMES}): aumente `step`", v.len())));
    }
    Ok(v)
}

/// Monta a grade: cada imagem reduzida a `thumb_w` de largura, com o rótulo embaixo, e um título no topo.
pub fn contact_sheet(frames: &[(Image, String)], columns: u32, thumb_w: u32, title: &str) -> Image {
    let font = default_font();
    let bold = heavy_font();
    let cols = columns.clamp(1, 12).min(frames.len().max(1) as u32);
    let rows = (frames.len() as u32).div_ceil(cols).max(1);
    let (src_w, src_h) = frames.first().map(|(i, _)| (i.width, i.height)).unwrap_or((16, 9));
    let s = thumb_w as f32 / src_w as f32;
    let th = ((src_h as f32 * s).round() as u32).max(1);
    let (gap, label_h, top) = (10u32, 26u32, 48u32);
    let w = cols * thumb_w + (cols + 1) * gap;
    let h = top + rows * (th + label_h + gap) + gap;
    let mut sheet = Image::filled(w, h, [0.95, 0.95, 0.96, 1.0]);
    fill_rect(&mut sheet, 0, 0, w as i32, top as i32 - 8, [0.14, 0.17, 0.26, 1.0]);
    draw_text(&mut sheet, bold, 22.0, title, gap as f32, 8.0, [1.0, 1.0, 1.0, 1.0]);
    for (k, (img, label)) in frames.iter().enumerate() {
        let (c, r) = (k as u32 % cols, k as u32 / cols);
        let x = (gap + c * (thumb_w + gap)) as i32;
        let y = (top + r * (th + label_h + gap)) as i32;
        let small = transform_image(img, Affine::from_trs(0.0, 0.0, s, 0.0, [0.0, 0.0]), thumb_w, th);
        sheet.draw_over(&small, x, y, 1.0);
        draw_text(&mut sheet, font, 16.0, label, x as f32 + 2.0, (y + th as i32 + 3) as f32, [0.15, 0.17, 0.22, 1.0]);
    }
    sheet
}

fn label(frame: u32, fps: f64) -> String {
    format!("q{frame}  {:.2} s", frame as f64 / fps)
}

/// Folha de contato de um corte (render final com câmera, ou o canvas inteiro com `camera = false`).
pub fn cut_contact_sheet(p: &Project, blobs: &BlobStore, cut: &Id, frames: &[u32], columns: u32, thumb_w: u32, camera: bool) -> ToolResult<Image> {
    let cd = cut_data(p, cut)?;
    if let Some(f) = frames.iter().find(|f| **f >= cd.duration) {
        return Err(ToolError::Invalid(format!("quadro {f} fora do corte (duração {})", cd.duration)));
    }
    let opts = RenderOptions { background: Some([1.0, 1.0, 1.0]), camera, ..Default::default() };
    let fps = p.settings.fps();
    let mut imgs = Vec::with_capacity(frames.len());
    for &f in frames {
        let out = render_frame(p, blobs, cut, f, &opts).map_err(|e| ToolError::Render(e.to_string()))?;
        imgs.push((out.image, label(f, fps)));
    }
    let name = node(p, cut)?.name.clone();
    Ok(contact_sheet(&imgs, columns, thumb_w, &format!("{name} — {} quadros", frames.len())))
}

/// Folha de contato da edição (legendas queimadas, transições, créditos) nos quadros globais `frames`.
pub fn edit_contact_sheet(p: &Project, blobs: &BlobStore, doc: &EditDoc, frames: &[u32], columns: u32, thumb_w: u32, burn_subtitles: bool) -> ToolResult<Image> {
    let fps = p.settings.fps();
    let total = doc.timeline(p, fps as f32)?.total;
    if let Some(f) = frames.iter().find(|f| **f >= total) {
        return Err(ToolError::Invalid(format!("quadro {f} fora da edição ({total} quadros)")));
    }
    let mut imgs = Vec::with_capacity(frames.len());
    for &f in frames {
        imgs.push((render_edit_frame(p, blobs, doc, f, burn_subtitles)?, label(f, fps)));
    }
    Ok(contact_sheet(&imgs, columns, thumb_w, &format!("Edição — {total} quadros ({:.1} s)", total as f64 / fps)))
}
