//! Pincéis de cenário de duas cores (auditoria 4.9.3): a rosa tem luz nas pétalas e sombra nos vincos, a nuvem tem
//! topo claro e base na sombra sem escamas dentro, e a segunda cor segue a cor do pincel.
//! `AIAI_BRUSH_SHEET=<arquivo.png>` grava a prancha para conferir a olho.

use aiai_paint::brush::{BrushPoint, BrushPreset, paint_stroke};
use aiai_paint::image::Image;

fn preset(name: &str) -> BrushPreset {
    BrushPreset::defaults().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("preset {name}"))
}

fn line(x0: f32, x1: f32, y: f32, wave: f32) -> Vec<BrushPoint> {
    (0..80).map(|k| {
        let t = k as f32 / 79.0;
        BrushPoint { x: x0 + (x1 - x0) * t, y: y + wave * (t * std::f32::consts::TAU).sin(), pressure: 0.7 + 0.3 * (t * std::f32::consts::PI).sin() }
    }).collect()
}

fn luma(p: [f32; 4]) -> f32 {
    if p[3] <= 0.0 { return 0.0 }
    (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / p[3]
}

#[test]
fn rose_and_cloud_have_two_tones_and_follow_the_brush_color() {
    let (w, h) = (900u32, 560u32);
    let mut sheet = Image::new(w, h);
    for p in sheet.data.iter_mut() {
        *p = [0.93, 0.95, 0.9, 1.0];
    }
    // céu atrás da nuvem
    for y in 300..h {
        for x in 0..w {
            let i = sheet.idx(x, y);
            sheet.data[i] = [0.45, 0.66, 0.9, 1.0];
        }
    }
    let red = [0.82, 0.16, 0.22, 1.0];
    let white = [1.0, 1.0, 1.0, 1.0];
    let green = [0.35, 0.62, 0.28, 1.0];
    paint_stroke(&mut sheet, &preset("Rosas"), &line(40.0, 420.0, 70.0, 12.0), red, 3);
    paint_stroke(&mut sheet, &preset("Rosas"), &line(480.0, 860.0, 70.0, 12.0), [0.98, 0.86, 0.9, 1.0], 4);
    paint_stroke(&mut sheet, &preset("Folhagem"), &line(40.0, 420.0, 190.0, 15.0), green, 5);
    paint_stroke(&mut sheet, &preset("Grama"), &line(480.0, 860.0, 210.0, 0.0), green, 6);
    paint_stroke(&mut sheet, &preset("Nuvem"), &line(80.0, 820.0, 420.0, 30.0), white, 7);
    // a mesma rosa em três tamanhos (carimbo pequeno, médio, grande)
    for (k, size) in [18.0f32, 40.0, 90.0].iter().enumerate() {
        let mut big = preset("Rosas");
        big.size = *size;
        big.scatter = 0.0;
        big.count = 1;
        big.spacing = 2.0;
        // a forma da ponta em si (o preset espalha rosas menores pelo spray desde o S25)
        big.spray = None;
        paint_stroke(&mut sheet, &big, &[BrushPoint { x: 470.0 + 90.0 * k as f32, y: 140.0, pressure: 1.0 }], red, 9);
    }
    if let Ok(path) = std::env::var("AIAI_BRUSH_SHEET") {
        sheet.save_png(std::path::Path::new(&path)).unwrap();
    }

    // uma rosa sozinha: as duas cores aparecem (luz e sombra), e a sombra é a cor do pincel escurecida
    let mut img = Image::new(120, 120);
    let mut one = preset("Rosas");
    one.size = 100.0;
    one.scatter = 0.0;
    one.count = 1;
    one.size_jitter = 0.0;
    one.angle_jitter = 0.0;
    one.value_jitter = 0.0;
    one.spray = None;
    paint_stroke(&mut img, &one, &[BrushPoint { x: 60.0, y: 60.0, pressure: 1.0 }], red, 1);
    let lum: Vec<f32> = img.data.iter().filter(|p| p[3] > 0.99).map(|p| luma(*p)).collect();
    assert!(lum.len() > 4000, "botão cheio, sem buracos: {} px", lum.len());
    let (lo, hi) = lum.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(hi - lo > 0.12, "luz e sombra na mesma rosa: {lo}..{hi}");
    let center = img.data[img.idx(60, 60)];
    assert!(center[0] > center[1] * 2.0 && luma(center) < 0.6 * luma(red), "miolo é vinho (vermelho escuro): {center:?}");

    // nuvem: no topo de um traço reto, da esquerda para a direita, o tom não sobe e desce em escamas
    let mut sky = Image::new(700, 200);
    let mut c = preset("Nuvem");
    c.scatter = 0.0;
    c.size_jitter = 0.0;
    paint_stroke(&mut sky, &c, &line(80.0, 620.0, 100.0, 0.0), white, 2);
    let row: Vec<f32> = (150..550).map(|x| luma(sky.data[sky.idx(x, 82)])).collect();
    let worst = row.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
    assert!(worst < 0.02, "sem degraus de carimbo no corpo da nuvem: salto {worst}");
    let (top, base) = (luma(sky.data[sky.idx(350, 70)]), luma(sky.data[sky.idx(350, 128)]));
    assert!(top > 0.95 && base < top - 0.1, "topo claro, base na sombra: {top} / {base}");
}
