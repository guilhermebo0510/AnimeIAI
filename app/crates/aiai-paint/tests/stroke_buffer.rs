//! O traço alimentado aos pedaços (prévia ao vivo do app) dá o mesmo resultado que o caminho inteiro de uma vez (commit).

use aiai_paint::brush::{BrushPoint, BrushPreset, DualBrush, StrokeBuffer, paint_stroke_at};
use aiai_paint::image::Image;

fn path() -> Vec<BrushPoint> {
    (0..240)
        .map(|k| {
            let t = k as f32 / 239.0;
            BrushPoint { x: 30.0 + 540.0 * t, y: 160.0 + 90.0 * (t * 9.0).sin(), pressure: 0.3 + 0.7 * (t * 3.1).sin().abs() }
        })
        .collect()
}

fn base() -> Image {
    // camada com conteúdo (o borrador arrasta o que já está lá)
    let mut img = Image::new(600, 320);
    for y in 0..320u32 {
        for x in 0..600u32 {
            if (x / 40 + y / 40) % 2 == 0 {
                let i = img.idx(x, y);
                img.data[i] = [0.2, 0.4, 0.8, 1.0];
            }
        }
    }
    img
}

#[test]
fn pieces_and_whole_paint_the_same() {
    let mut presets: Vec<BrushPreset> = BrushPreset::defaults().into_iter().filter(|p| ["Grama", "Aquarela", "Borrador", "Rosas", "Folhagem", "Carvão"].contains(&p.name.as_str())).collect();
    let mut dual = BrushPreset::round("duplo", 30.0, 0.8);
    dual.dual = Some(DualBrush { size: 12.0, spacing: 0.3, scatter: 0.5, hardness: 0.6 });
    presets.push(dual);
    assert_eq!(presets.len(), 7);
    let color = [0.85, 0.3, 0.2, 1.0];
    for p in &presets {
        let mut whole = base();
        paint_stroke_at(&mut whole, [0, 0], p, &path(), color, 42).unwrap();
        // em pedaços de tamanhos variados, como chegam do mouse
        let layer = base();
        let mut sb = StrokeBuffer::new(p, color, 42, layer.width, layer.height, [0, 0]);
        let pts = path();
        let mut i = 0;
        for (k, n) in [1usize, 3, 7, 2, 25, 1, 60].iter().cycle().enumerate() {
            if i >= pts.len() {
                break;
            }
            let j = (i + n).min(pts.len());
            sb.add(Some(&layer), &pts[i..j]);
            // uma prévia no meio (clone) não muda o traço de verdade
            if k % 3 == 0 {
                let mut preview = sb.clone();
                preview.finish(Some(&layer));
                let mut tmp = layer.clone();
                preview.composite(&mut tmp, [0, 0]);
            }
            i = j;
        }
        sb.finish(Some(&layer));
        let mut pieces = base();
        sb.composite(&mut pieces, [0, 0]).unwrap();
        let worst = whole.data.iter().zip(&pieces.data).map(|(a, b)| (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
        assert!(worst < 1e-6, "{}: diferença {worst}", p.name);
        // compor por retângulos (a prévia recompõe só o que mudou) dá o mesmo que compor tudo
        if p.dual.is_none() {
            let mut tiled = base();
            for ty in (0..320).step_by(70) {
                for tx in (0..600).step_by(90) {
                    sb.composite_rect(&mut tiled, [0, 0], [tx, ty, (tx + 90).min(600), (ty + 70).min(320)]);
                }
            }
            let worst = tiled.data.iter().zip(&pieces.data).map(|(a, b)| (0..4).map(|c| (a[c] - b[c]).abs()).fold(0.0f32, f32::max)).fold(0.0f32, f32::max);
            assert!(worst < 1e-5, "{} por retângulos: diferença {worst}", p.name);
        }
    }
}
