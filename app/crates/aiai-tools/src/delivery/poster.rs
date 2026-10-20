//! Quadros da fonte de uma entrega: miniatura (cartão de entregável), pôster (quadro escolhido, com título opcional sobre a
//! imagem, 1280×720 estilo YouTube) e folha de contato. O desenho é o mesmo render do vídeo exportado.

use super::{SourceKind, SourceRef};
use crate::context::{ToolError, ToolResult, cut_data, node};
use crate::edit::{draw_text, load_edit, render_edit_frame};
use crate::preview::{MAX_SHEET_FRAMES, contact_sheet, cut_contact_sheet, edit_contact_sheet};
use aiai_core::{BlobStore, Id, Project};
use aiai_paint::{Affine, Image, transform_image};
use aiai_render::{RenderOptions, render_frame};

/// Largura da miniatura do cartão.
pub const THUMB_WIDTH: u32 = 320;
/// Largura do pôster (1280×720 nos projetos 16:9).
pub const POSTER_WIDTH: u32 = 1280;

/// Quadro do pôster quando ninguém escolheu: o do meio da fonte.
pub fn default_frame(p: &Project, source: &SourceRef) -> u32 {
    source.total_frames(p).map(|t| t / 2).unwrap_or(0)
}

/// Reduz a imagem à largura dada (nunca amplia).
pub fn scale_to_width(img: &Image, w: u32) -> Image {
    if w >= img.width || w == 0 {
        return img.clone();
    }
    let s = w as f32 / img.width as f32;
    let h = ((img.height as f32 * s).round() as u32).max(1);
    transform_image(img, Affine::from_trs(0.0, 0.0, s, 0.0, [0.0, 0.0]), w, h)
}

fn white_opts() -> RenderOptions {
    RenderOptions { background: Some([1.0, 1.0, 1.0]), camera: true, ..Default::default() }
}

/// O quadro `frame` (contado a partir do começo da fonte) renderizado como o vídeo o desenharia.
pub fn source_frame(p: &Project, blobs: &BlobStore, source: &SourceRef, frame: u32) -> ToolResult<Image> {
    let total = source.total_frames(p)?;
    if frame >= total {
        return Err(ToolError::Invalid(format!("quadro {frame} fora da fonte ({total} quadros)")));
    }
    match source.kind {
        SourceKind::Edit => render_edit_frame(p, blobs, &load_edit(p, &source.id)?, frame, true),
        _ => {
            let mut left = frame;
            for (cut, s, e) in source.segments(p)? {
                let n = e - s;
                if left < n {
                    return render_frame(p, blobs, &cut, s + left, &white_opts()).map(|o| o.image).map_err(|e| ToolError::Render(e.to_string()));
                }
                left -= n;
            }
            Err(ToolError::Invalid(format!("quadro {frame} fora da fonte")))
        }
    }
}

/// Escurece o terço de baixo e escreve o título (e a legenda) sobre a imagem.
pub fn add_title(img: &mut Image, title: &str, subtitle: &str) {
    if title.trim().is_empty() && subtitle.trim().is_empty() {
        return;
    }
    let (w, h) = (img.width, img.height);
    let y0 = (h as f32 * 0.55) as u32;
    for y in y0..h {
        let k = ((y - y0) as f32 / (h - y0).max(1) as f32).powf(1.4) * 0.78;
        for x in 0..w {
            let i = img.idx(x, y);
            for c in 0..3 {
                img.data[i][c] *= 1.0 - k;
            }
        }
    }
    let size = (h as f32 * 0.105).max(14.0);
    let outline = Some(([0.0, 0.0, 0.0, 0.85], (h as f32 * 0.006).max(1.0)));
    let has_sub = !subtitle.trim().is_empty();
    let ty = if has_sub { h as f32 * 0.78 } else { h as f32 * 0.84 };
    // título: encolhe até caber na largura (uma linha)
    let fit = |text: &str, base: f32| -> f32 {
        let chars = text.chars().count().max(1) as f32;
        base.min(w as f32 * 0.92 / (chars * 0.56))
    };
    draw_text(img, title.trim(), [w as f32 / 2.0, ty], fit(title, size), [1.0, 1.0, 1.0, 1.0], outline);
    if has_sub {
        let ss = size * 0.5;
        draw_text(img, subtitle.trim(), [w as f32 / 2.0, h as f32 * 0.9], fit(subtitle, ss), [0.95, 0.95, 0.95, 1.0], outline);
    }
}

/// Pôster: o quadro escolhido na largura pedida, com título e legenda opcionais.
pub fn poster(p: &Project, blobs: &BlobStore, source: &SourceRef, frame: u32, width: u32, title: &str, subtitle: &str) -> ToolResult<Image> {
    let mut img = scale_to_width(&source_frame(p, blobs, source, frame)?, width);
    add_title(&mut img, title, subtitle);
    Ok(img)
}

/// Miniatura do cartão (sem título).
pub fn thumbnail(p: &Project, blobs: &BlobStore, source: &SourceRef, frame: Option<u32>) -> ToolResult<Image> {
    poster(p, blobs, source, frame.unwrap_or_else(|| default_frame(p, source)), THUMB_WIDTH, "", "")
}

/// Até 12 quadros espaçados por igual (no mínimo 1).
fn spread(total: u32, n: u32) -> Vec<u32> {
    let n = n.clamp(1, MAX_SHEET_FRAMES as u32).min(total.max(1));
    (0..n).map(|k| ((k as u64 * total as u64) / n as u64) as u32 + total / (2 * n)).map(|f| f.min(total.saturating_sub(1))).collect()
}

/// Folha de contato da fonte: 12 quadros espaçados (corte e edição) ou o quadro do meio de cada corte (episódio).
pub fn contact(p: &Project, blobs: &BlobStore, source: &SourceRef) -> ToolResult<Image> {
    match source.kind {
        SourceKind::Edit => {
            let doc = load_edit(p, &source.id)?;
            let total = source.total_frames(p)?;
            edit_contact_sheet(p, blobs, &doc, &spread(total, 12), 4, 480, true)
        }
        SourceKind::Cut => {
            let (cut, s, e) = source.segments(p)?.remove(0);
            let frames: Vec<u32> = spread(e - s, 12).into_iter().map(|f| s + f).collect();
            cut_contact_sheet(p, blobs, &cut, &frames, 4, 480, true)
        }
        SourceKind::Episode => {
            let mut imgs = Vec::new();
            for (cut, _, _) in source.segments(p)?.into_iter().take(MAX_SHEET_FRAMES) {
                let dur = cut_data(p, &cut)?.duration;
                let img = crate::production::cut_thumbnail(p, blobs, &cut, dur / 2, 480)?;
                imgs.push((img, node(p, &cut)?.name.clone()));
            }
            Ok(contact_sheet(&imgs, 4, 480, &format!("{} — {} cortes", node(p, &source.id)?.name, imgs.len())))
        }
    }
}

/// Corte e quadro local de um quadro global da fonte (para abrir o corte certo ao corrigir).
pub fn locate(p: &Project, source: &SourceRef, frame: u32) -> Option<(Id, u32)> {
    if source.kind == SourceKind::Edit {
        return None;
    }
    let mut left = frame;
    for (cut, s, e) in source.segments(p).ok()? {
        if left < e - s {
            return Some((cut, s + left));
        }
        left -= e - s;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::testkit::session_with_cut;
    use super::*;

    #[test]
    fn thumbnails_and_posters_come_from_the_same_render_as_the_video() {
        let (s, ep, cut) = session_with_cut(24);
        let src = SourceRef::cut(cut.clone());
        let t = thumbnail(&s.project, &s.blobs, &src, None).unwrap();
        assert_eq!((t.width, t.height), (64, 36), "o projeto de teste é menor que a miniatura: não amplia");
        // o quadro da miniatura é o do render do corte
        let direct = render_frame(&s.project, &s.blobs, &cut, 12, &white_opts()).unwrap().image;
        assert_eq!(t.content_hash(), direct.content_hash());
        // episódio: o quadro global cai no corte certo
        let e = source_frame(&s.project, &s.blobs, &SourceRef::episode(ep.clone()), 5).unwrap();
        assert_eq!(e.content_hash(), render_frame(&s.project, &s.blobs, &cut, 5, &white_opts()).unwrap().image.content_hash());
        assert!(source_frame(&s.project, &s.blobs, &src, 24).is_err());
        assert_eq!(locate(&s.project, &SourceRef::episode(ep), 7), Some((cut.clone(), 7)));
        // trecho de um corte: o quadro 0 da fonte é o `start`
        let mut part = SourceRef::cut(cut.clone());
        part.start = Some(6);
        part.end = Some(18);
        let f0 = source_frame(&s.project, &s.blobs, &part, 0).unwrap();
        assert_eq!(f0.content_hash(), render_frame(&s.project, &s.blobs, &cut, 6, &white_opts()).unwrap().image.content_hash());
        assert_eq!(default_frame(&s.project, &part), 6);
    }

    #[test]
    fn the_title_darkens_the_bottom_and_draws_text() {
        let (s, _ep, cut) = session_with_cut(12);
        let src = SourceRef::cut(cut);
        let plain = poster(&s.project, &s.blobs, &src, 3, 64, "", "").unwrap();
        let titled = poster(&s.project, &s.blobs, &src, 3, 64, "Última Rosa", "Episódio 1").unwrap();
        assert_ne!(plain.content_hash(), titled.content_hash());
        // o topo não muda; a base fica mais escura
        let top = (plain.get(10, 2), titled.get(10, 2));
        assert_eq!(top.0, top.1);
        let (a, b) = (plain.get(2, 34), titled.get(2, 34));
        assert!(b[0] + b[1] + b[2] < a[0] + a[1] + a[2], "{a:?} {b:?}");
    }

    #[test]
    fn contact_sheets_cover_cuts_edits_and_episodes() {
        let (s, ep, cut) = session_with_cut(24);
        let c = contact(&s.project, &s.blobs, &SourceRef::cut(cut.clone())).unwrap();
        assert!(c.width > 480 && c.height > 100);
        assert!(contact(&s.project, &s.blobs, &SourceRef::episode(ep)).unwrap().width > 480);
        assert_eq!(spread(24, 12).len(), 12);
        assert_eq!(spread(3, 12), vec![0, 1, 2]);
        assert!(spread(100, 12).iter().all(|f| *f < 100));
    }
}
