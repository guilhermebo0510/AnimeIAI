//! Pincéis que misturam e misturadores do S25: tinta que mistura com a camada (`paint_mix`), troca do pixel nos
//! misturadores e no Desfocar, espalhamento do spray e os presets de fábrica.

use aiai_paint::Image;
use aiai_paint::brush::{BrushPoint, BrushPreset, paint_stroke};

const RED: [f32; 4] = [220.0 / 255.0, 40.0 / 255.0, 40.0 / 255.0, 1.0];
const BLUE: [f32; 4] = [40.0 / 255.0, 70.0 / 255.0, 220.0 / 255.0, 1.0];

fn line(x0: f32, y0: f32, x1: f32, y1: f32, step: f32) -> Vec<BrushPoint> {
    let n = ((x1 - x0).hypot(y1 - y0) / step).round().max(1.0) as usize;
    (0..=n).map(|i| {
        let t = i as f32 / n as f32;
        BrushPoint { x: x0 + (x1 - x0) * t, y: y0 + (y1 - y0) * t, pressure: 1.0, tilt: None }
    }).collect()
}

fn preset(name: &str, size: f32) -> BrushPreset {
    let mut p = BrushPreset::defaults().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("preset {name}"));
    p.size = size;
    p
}

/// Faixa horizontal vermelha opaca (y 40..60) numa camada vazia de 120×100.
fn red_band() -> Image {
    let mut img = Image::new(120, 100);
    for y in 40..60 {
        for x in 0..120 {
            let i = img.idx(x, y);
            img.data[i] = RED;
        }
    }
    img
}

/// Quanto a cor (não pré-multiplicada) puxa para o vermelho na reta azul→vermelho (0 = azul, 1 = vermelho).
fn pull_red(c: [f32; 4]) -> f32 {
    let u = [c[0] / c[3], c[1] / c[3], c[2] / c[3]];
    let d = [RED[0] - BLUE[0], RED[1] - BLUE[1], RED[2] - BLUE[2]];
    let v = [u[0] - BLUE[0], u[1] - BLUE[1], u[2] - BLUE[2]];
    (v[0] * d[0] + v[1] * d[1] + v[2] * d[2]) / (d[0] * d[0] + d[1] * d[1] + d[2] * d[2])
}

#[test]
fn factory_paint_brushes_and_blenders_load_round_trip_and_are_grouped() {
    let pincel = ["Caneta pincel", "Nanquim seco", "Nanquim aguado", "Nanquim de mancha úmida", "Aquarela redonda", "Aquarela chata", "Transbordo suave", "Lavagem molhada", "Lavagem áspera", "Respingos de aquarela", "Pincel redondo de mistura", "Guache", "Guache seco", "Guache fino", "Óleo", "Óleo grosso", "Pontilhismo", "Pintar e aplicar"];
    let mistura = ["Misturar", "Desfocar", "Ponta do dedo", "Misturador de pintura", "Misturador molhado", "Misturador texturizado", "Pincel aquoso", "Misturador de guache"];
    let all = BrushPreset::defaults();
    for (group, names) in [("pincel", &pincel[..]), ("mistura", &mistura[..])] {
        for n in names {
            let p = all.iter().find(|p| p.name == *n).unwrap_or_else(|| panic!("{n}"));
            assert_eq!(p.group, group, "{n}");
            p.validate().unwrap_or_else(|e| panic!("{n}: {e}"));
            let back: BrushPreset = serde_json::from_str(&serde_json::to_string(p).unwrap()).unwrap();
            assert_eq!(&back, p, "{n}");
        }
    }
    // os misturadores não põem cor do pincel
    for n in mistura {
        let m = all.iter().find(|p| p.name == n).unwrap().paint_mix.clone().expect("misturador com paint_mix");
        assert_eq!(m.amount, 0.0, "{n}");
    }
}

#[test]
fn presets_without_the_new_fields_keep_their_json() {
    let p = BrushPreset::round("Velho", 20.0, 0.8);
    let j = serde_json::to_string(&p).unwrap();
    assert!(!j.contains("paint_mix"), "{j}");
    let s = preset("Spray", 30.0);
    let j = serde_json::to_string(&s).unwrap();
    assert!(!j.contains("spread"), "o spread 1 não vai para o JSON: {j}");
}

#[test]
fn blenders_never_paint_the_brush_color() {
    // passar um misturador com cor preta sobre a faixa vermelha não escurece nada (antes ficavam manchas pretas)
    for name in ["Misturar", "Desfocar", "Ponta do dedo", "Misturador de pintura", "Misturador molhado", "Pincel aquoso"] {
        let mut img = red_band();
        paint_stroke(&mut img, &preset(name, 30.0), &line(10.0, 50.0, 110.0, 50.0, 2.0), [0.0, 0.0, 0.0, 1.0], 3);
        for y in 0..100 {
            for x in 0..120 {
                let c = img.get(x, y);
                if c[3] > 0.05 {
                    // continua vermelho (não ficou cinza/preto): o canal vermelho domina
                    assert!(c[0] / c[3] > 0.6 && c[1] / c[3] < 0.35, "{name} em ({x},{y}): {c:?}");
                }
            }
        }
        // num canvas vazio não aparece nada
        let mut empty = Image::new(120, 100);
        paint_stroke(&mut empty, &preset(name, 30.0), &line(10.0, 50.0, 110.0, 50.0, 2.0), [0.0, 0.0, 0.0, 1.0], 3);
        assert!(empty.data.iter().all(|p| p[3] < 1e-4), "{name} pintou num canvas vazio");
    }
}

#[test]
fn blur_softens_the_band_edge_and_can_lower_alpha() {
    let mut img = red_band();
    // borda de cima da faixa (y = 40): pixel de dentro e de fora
    let before_out = img.get(60, 37)[3];
    paint_stroke(&mut img, &preset("Desfocar", 30.0), &line(10.0, 40.0, 110.0, 40.0, 2.0), [0.0, 0.0, 0.0, 1.0], 3);
    let after_out = img.get(60, 37)[3];
    let after_in = img.get(60, 42)[3];
    assert!(before_out == 0.0 && after_out > 0.05, "o vermelho espalha para fora: {after_out}");
    assert!(after_in < 0.999, "e a borda de dentro fica mais transparente (troca, não pinta por cima): {after_in}");
}

#[test]
fn mixing_brush_picks_some_red_and_full_paint_covers() {
    // guache azul cruzando a faixa vermelha: puxa um pouco de vermelho (quantidade 80%)
    let mut img = red_band();
    paint_stroke(&mut img, &preset("Guache", 20.0), &line(60.0, 5.0, 60.0, 95.0, 1.0), BLUE, 3);
    let cross = img.get(60, 50);
    let pr = pull_red(cross);
    assert!((0.05..0.45).contains(&pr), "guache no cruzamento: puxa {pr}");
    // guache fino (100% de tinta) cobre quase tudo de azul
    let mut img = red_band();
    paint_stroke(&mut img, &preset("Guache fino", 20.0), &line(60.0, 5.0, 60.0, 95.0, 1.0), BLUE, 3);
    let pr2 = pull_red(img.get(60, 50));
    assert!(pr2 < pr && pr2 < 0.15, "guache fino no cruzamento: {pr2}");
    // aquarela multiplica: o cruzamento fica mais escuro que o azul sozinho
    let mut img = red_band();
    paint_stroke(&mut img, &preset("Aquarela redonda", 20.0), &line(60.0, 5.0, 60.0, 95.0, 1.0), BLUE, 3);
    let c = img.get(60, 50);
    assert!(c[2] / c[3] < 0.5, "aquarela multiplica no cruzamento: {c:?}");
}

#[test]
fn spray_spread_scatters_beyond_the_tip() {
    let mut p = preset("Lavagem molhada", 20.0);
    let mut img = Image::new(200, 200);
    paint_stroke(&mut img, &p, &line(20.0, 100.0, 180.0, 100.0, 2.0), [0.0, 0.0, 0.0, 1.0], 5);
    let reach = |img: &Image| (0..200u32).filter(|y| (0..200u32).any(|x| img.get(x, *y)[3] > 0.05)).count();
    let wide = reach(&img);
    if let Some(s) = p.spray.as_mut() {
        s.spread = 1.0;
    }
    let mut img2 = Image::new(200, 200);
    paint_stroke(&mut img2, &p, &line(20.0, 100.0, 180.0, 100.0, 2.0), [0.0, 0.0, 0.0, 1.0], 5);
    let narrow = reach(&img2);
    assert!(wide as f32 > narrow as f32 * 1.4, "espalhado {wide} linhas × no disco {narrow}");
}
