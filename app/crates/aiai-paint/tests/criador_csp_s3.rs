//! Opções do criador de pincéis que vieram do estudo do Clip Studio (seção 3 do plano contínuo): pelo menos 1 px,
//! variação para a cor secundária e por traço, densidade pelo intervalo, rotação da textura, partículas alinhadas,
//! borda de aquarela (escurecer e largura), modos de mesclagem novos e "forma do pincel".

use aiai_paint::Image;
use aiai_paint::brush::{BrushMode, BrushPoint, BrushPreset, BrushTexture, BrushTip, TextureKind, TextureMode, flow_for_spacing, paint_stroke};
use aiai_paint::brush_dyn::Spray;

fn line(x0: f32, x1: f32, y: f32, step: f32, pressure: f32) -> Vec<BrushPoint> {
    let n = ((x1 - x0) / step).round() as usize;
    (0..=n).map(|i| BrushPoint { x: x0 + (x1 - x0) * i as f32 / n as f32, y, pressure, tilt: None }).collect()
}

fn alpha_sum(img: &Image) -> f32 {
    img.data.iter().map(|p| p[3]).sum()
}

/// Cor sem pré-multiplicar.
fn un(p: [f32; 4]) -> [f32; 3] {
    if p[3] <= 1e-6 { [0.0; 3] } else { [p[0] / p[3], p[1] / p[3], p[2] / p[3]] }
}

#[test]
fn at_least_one_pixel_keeps_a_light_pressure_stroke_visible() {
    let mut p = BrushPreset::round("fino", 2.0, 1.0);
    p.size_pressure_min = 0.0;
    // eixo no meio da linha de pixels (a ponta de 1 px num eixo inteiro cai entre dois centros)
    let pts = line(10.0, 110.0, 20.5, 1.0, 0.05);
    let mut thin = Image::new(120, 40);
    paint_stroke(&mut thin, &p, &pts, [0.0, 0.0, 0.0, 1.0], 1);
    p.min_one_px = true;
    p.validate().unwrap();
    let mut one = Image::new(120, 40);
    paint_stroke(&mut one, &p, &pts, [0.0, 0.0, 0.0, 1.0], 1);
    let (a, b) = (alpha_sum(&thin), alpha_sum(&one));
    assert!(b > a * 1.4, "com pelo menos 1 px o traço leve aparece: {a} -> {b}");
    // e cada coluna do meio do traço tem perto de 1 px de tinta
    let col: f32 = (0..40).map(|y| one.get(60, y)[3]).sum();
    assert!((0.4..=1.6).contains(&col), "~1 px de tinta por coluna: {col}");
}

#[test]
fn sub_color_jitter_pulls_dabs_toward_the_secondary_color() {
    let mut p = BrushPreset::round("var", 6.0, 1.0);
    p.spacing = 2.0; // carimbos separados
    p.sub_jitter = 1.0;
    p.sub_color = Some([0.0, 0.0, 1.0]);
    p.validate().unwrap();
    let mut img = Image::new(400, 30);
    paint_stroke(&mut img, &p, &line(10.0, 390.0, 15.0, 2.0, 1.0), [1.0, 0.0, 0.0, 1.0], 5);
    let blues: Vec<f32> = (0..400).filter(|&x| img.get(x, 15)[3] > 0.9).map(|x| un(img.get(x, 15))[2]).collect();
    assert!(blues.len() > 50);
    let (mn, mx) = blues.iter().fold((1.0f32, 0.0f32), |(a, b), &v| (a.min(v), b.max(v)));
    assert!(mn < 0.2 && mx > 0.6, "os carimbos vão do vermelho à secundária: {mn}..{mx}");
    // sem a cor secundária, nada muda
    let mut q = p.clone();
    q.sub_color = None;
    let mut plain = Image::new(400, 30);
    paint_stroke(&mut plain, &q, &line(10.0, 390.0, 15.0, 2.0, 1.0), [1.0, 0.0, 0.0, 1.0], 5);
    assert!((0..400).filter(|&x| plain.get(x, 15)[3] > 0.9).all(|x| un(plain.get(x, 15))[2] < 1e-4));
}

#[test]
fn per_stroke_jitter_paints_the_whole_stroke_in_one_color() {
    let mut p = BrushPreset::round("hue", 6.0, 1.0);
    p.spacing = 2.0;
    p.hue_jitter = 0.5;
    p.jitter_per_stroke = true;
    let pts = line(10.0, 390.0, 15.0, 2.0, 1.0);
    let colors = |seed: u64| {
        let mut img = Image::new(400, 30);
        paint_stroke(&mut img, &p, &pts, [0.9, 0.3, 0.2, 1.0], seed);
        (0..400).filter(|&x| img.get(x, 15)[3] > 0.95).map(|x| un(img.get(x, 15))).collect::<Vec<_>>()
    };
    let a = colors(3);
    let first = a[0];
    assert!(a.iter().all(|c| (0..3).all(|k| (c[k] - first[k]).abs() < 0.02)), "uma cor só no traço");
    // outros traços, outras cores
    let firsts: Vec<[f32; 3]> = [11u64, 23, 37, 51].iter().map(|&s| colors(s)[0]).collect();
    let far = firsts.iter().filter(|c| (0..3).any(|k| (c[k] - first[k]).abs() > 0.05)).count();
    assert!(far >= 2, "a cor muda de um traço para outro: {first:?} {firsts:?}");
}

#[test]
fn density_by_gap_keeps_the_tone_when_spacing_changes() {
    assert!((flow_for_spacing(0.3, 0.1) - 0.3).abs() < 1e-6);
    assert!(flow_for_spacing(0.3, 0.4) > 0.6);
    assert!(flow_for_spacing(0.3, 0.05) < 0.3);
    // tom no meio de um traço macio com intervalo 10% e 40%: com a opção, ficam perto; sem, o de 40% é bem mais claro
    let tone = |spacing: f32, by_gap: bool| {
        let mut p = BrushPreset::round("t", 20.0, 0.0);
        p.flow = 0.15;
        p.spacing = spacing;
        p.flow_by_spacing = by_gap;
        let mut img = Image::new(300, 40);
        paint_stroke(&mut img, &p, &line(20.0, 280.0, 20.0, 1.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1);
        (100..200).map(|x| img.get(x, 20)[3]).sum::<f32>() / 100.0
    };
    let (a, b) = (tone(0.1, true), tone(0.4, true));
    assert!((a - b).abs() < 0.12, "pelo intervalo o tom fica: {a} x {b}");
    let (c, d) = (tone(0.1, false), tone(0.4, false));
    assert!(c - d > 0.15, "sem a opção, intervalo maior clareia: {c} x {d}");
}

#[test]
fn texture_rotation_turns_the_pattern() {
    let paint = |rot: f32| {
        let mut p = BrushPreset::round("tx", 80.0, 1.0);
        p.texture = Some(BrushTexture { kind: TextureKind::Canvas, scale: 6.0, depth: 1.0, mode: TextureMode::Multiply, seed: 2, invert: false, rotation: rot, image: None, image_mask: None });
        p.validate().unwrap();
        let mut img = Image::new(160, 120);
        paint_stroke(&mut img, &p, &line(40.0, 120.0, 60.0, 2.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1);
        img
    };
    let (a, b) = (paint(0.0), paint(45.0));
    let diff: f32 = a.data.iter().zip(&b.data).map(|(x, y)| (x[3] - y[3]).abs()).sum::<f32>() / a.data.len() as f32;
    assert!(diff > 0.01, "a textura girada é outra: {diff}");
    // girar 360° volta ao mesmo
    let c = paint(360.0);
    let back: f32 = a.data.iter().zip(&c.data).map(|(x, y)| (x[3] - y[3]).abs()).fold(0.0, f32::max);
    assert!(back < 0.02, "{back}");
}

#[test]
fn aligned_particles_follow_the_stroke() {
    // partículas bem achatadas: alinhadas ao traço horizontal, a tinta se espalha mais em x que em y
    let spread = |align: bool| {
        let mut p = BrushPreset::round("sp", 40.0, 1.0);
        p.follow_direction = true;
        p.spray = Some(Spray { count: 30.0, size: 0.4, sync: true, size_min: 1.0, roundness_min: 0.1, bias: 0.0, count_exp: 0.0, spread: 0.1, align });
        p.validate().unwrap();
        let mut img = Image::new(200, 100);
        paint_stroke(&mut img, &p, &[BrushPoint { x: 100.0, y: 50.0, pressure: 1.0, tilt: None }, BrushPoint { x: 101.0, y: 50.0, pressure: 1.0, tilt: None }], [0.0, 0.0, 0.0, 1.0], 4);
        let (mut sx, mut sy, mut s) = (0.0f32, 0.0f32, 0.0f32);
        for y in 0..100 {
            for x in 0..200 {
                let a = img.get(x, y)[3];
                sx += a * (x as f32 - 100.0).powi(2);
                sy += a * (y as f32 - 50.0).powi(2);
                s += a;
            }
        }
        (sx / s).sqrt() / (sy / s).sqrt()
    };
    let (aligned, random) = (spread(true), spread(false));
    assert!(aligned > 1.12, "alinhadas: mais largas que altas ({aligned})");
    assert!(random < 1.05 && random < aligned - 0.1, "sorteadas: redondas no conjunto ({random} x {aligned})");
}

#[test]
fn watercolor_edge_darkness_and_width() {
    let wc = |dark: f32, width: f32| {
        let mut p = BrushPreset::round("aq", 40.0, 0.8);
        p.wet_edges = 0.8;
        p.wet_darkness = dark;
        p.wet_width = width;
        p.validate().unwrap();
        let mut img = Image::new(240, 100);
        paint_stroke(&mut img, &p, &line(40.0, 200.0, 50.0, 2.0, 1.0), [0.4, 0.6, 0.9, 1.0], 1);
        img
    };
    let (plain, dark) = (wc(0.0, 0.0), wc(1.0, 0.0));
    // a borda (perto do raio) fica mais escura; o meio quase não muda
    let edge_y = (30..50).find(|&y| plain.get(120, y)[3] > 0.05).unwrap();
    let (e0, e1) = (un(plain.get(120, edge_y + 2)), un(dark.get(120, edge_y + 2)));
    assert!(e1[2] < e0[2] - 0.08, "borda escurece: {e0:?} -> {e1:?}");
    let (m0, m1) = (un(plain.get(120, 50)), un(dark.get(120, 50)));
    assert!((m0[2] - m1[2]).abs() < 0.05, "o meio fica: {m0:?} x {m1:?}");
    // largura maior: a faixa da borda é outra
    let wide = wc(0.0, 10.0);
    let d: f32 = plain.data.iter().zip(&wide.data).map(|(x, y)| (x[3] - y[3]).abs()).sum();
    assert!(d > 5.0, "largura da borda muda o resultado: {d}");
}

#[test]
fn new_blend_modes_paint_like_the_layer_modes() {
    let base = [0.8, 0.2, 0.2, 1.0];
    let mut p = BrushPreset::round("m", 30.0, 1.0);
    p.mode = BrushMode::Darken;
    let mut img = Image::filled(100, 60, base);
    paint_stroke(&mut img, &p, &line(20.0, 80.0, 30.0, 2.0, 1.0), [0.5, 0.5, 0.5, 1.0], 1);
    let c = img.get(50, 30);
    assert!((c[0] - 0.5).abs() < 0.02 && (c[1] - 0.2).abs() < 0.02, "escurecer: o menor de cada canal: {c:?}");
    p.mode = BrushMode::Lighten;
    let mut img = Image::filled(100, 60, base);
    paint_stroke(&mut img, &p, &line(20.0, 80.0, 30.0, 2.0, 1.0), [0.5, 0.5, 0.5, 1.0], 1);
    let c = img.get(50, 30);
    assert!((c[0] - 0.8).abs() < 0.02 && (c[1] - 0.5).abs() < 0.02, "clarear: o maior: {c:?}");
    for m in [BrushMode::SoftLight, BrushMode::HardLight, BrushMode::Subtract, BrushMode::Difference, BrushMode::Hue, BrushMode::Saturation, BrushMode::Color, BrushMode::Luminosity] {
        p.mode = m;
        let back: BrushPreset = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.mode, m);
        let mut img = Image::filled(100, 60, base);
        let before = img.content_hash();
        paint_stroke(&mut img, &p, &line(20.0, 80.0, 30.0, 2.0, 1.0), [0.1, 0.7, 0.9, 1.0], 1);
        assert_ne!(img.content_hash(), before, "{m:?} pinta");
    }
}

#[test]
fn brush_shape_copies_the_tip_but_not_the_ink() {
    let src = BrushPreset::defaults().into_iter().find(|p| p.spray.is_some() && p.texture.is_none()).expect("um pincel de spray");
    let mut dst = BrushPreset::round("meu", 33.0, 0.5);
    dst.opacity = 0.4;
    dst.hue_jitter = 0.2;
    dst.apply_shape(&src);
    assert_eq!(dst.tip, src.tip);
    assert_eq!(dst.spray, src.spray);
    assert_eq!(dst.spacing, src.spacing);
    assert_eq!((dst.size, dst.opacity, dst.hue_jitter, dst.name.as_str()), (33.0, 0.4, 0.2, "meu"));
    dst.validate().unwrap();
    assert!(matches!(dst.tip, BrushTip::Round { .. } | BrushTip::Shape { .. } | BrushTip::Image { .. }));
}

#[test]
fn new_options_round_trip_and_stay_out_of_old_files() {
    let p = BrushPreset::round("x", 10.0, 0.5);
    let js = serde_json::to_string(&p).unwrap();
    for k in ["screen_size", "min_one_px", "sub_jitter", "sub_color", "jitter_per_stroke", "flow_by_spacing", "wet_darkness", "wet_width"] {
        assert!(!js.contains(k), "{k} não sai quando desligado");
    }
    let mut q = p.clone();
    q.screen_size = true;
    q.min_one_px = true;
    q.sub_jitter = 0.3;
    q.sub_color = Some([0.1, 0.2, 0.3]);
    q.jitter_per_stroke = true;
    q.flow_by_spacing = true;
    q.wet_darkness = 0.5;
    q.wet_width = 6.0;
    q.validate().unwrap();
    let back: BrushPreset = serde_json::from_str(&serde_json::to_string(&q).unwrap()).unwrap();
    assert_eq!(back, q);
    q.wet_width = 100.0;
    assert!(q.validate().is_err());
}

#[test]
fn field_names_list_every_preset_field() {
    let f = BrushPreset::field_names();
    for k in ["tip", "size", "spray", "paint_mix", "grain", "sub_jitter", "wet_width", "screen_size", "max_ink"] {
        assert!(f.iter().any(|x| x == k), "{k} em {f:?}");
    }
}

#[test]
fn tiny_classic_tip_does_not_vanish_on_a_pixel_border() {
    // eixo exatamente na divisa entre duas linhas de pixels (y inteiro): a ponta de ~1 px não pode sumir
    let mut p = BrushPreset::round("fino", 1.0, 0.95);
    p.size_pressure_min = 1.0;
    for y in [20.0f32, 20.25, 20.5] {
        let mut img = Image::new(120, 40);
        paint_stroke(&mut img, &p, &line(10.0, 110.0, y, 1.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1);
        let col: f32 = (0..40).map(|yy| img.get(60, yy)[3]).sum();
        assert!((0.6..=1.4).contains(&col), "y={y}: a linha fina tem ~1 px de tinta ({col})");
    }
}
