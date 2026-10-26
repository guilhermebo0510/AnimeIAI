//! Lápis, aerógrafos e borrachas do S25: grão de papel, spray, ponta de bastão, ponta macia com cauda e os presets.

use aiai_paint::Image;
use aiai_paint::brush::{BrushMode, BrushPoint, BrushPreset, BrushTip, TipShape, paint_stroke};
use aiai_paint::brush_dyn::{Grain, GrainKind, Spray};

fn line(x0: f32, x1: f32, y: f32, step: f32) -> Vec<BrushPoint> {
    let n = ((x1 - x0) / step).round() as usize;
    (0..=n).map(|i| BrushPoint { x: x0 + (x1 - x0) * i as f32 / n as f32, y, pressure: 1.0 }).collect()
}

fn preset(name: &str) -> BrushPreset {
    BrushPreset::defaults().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("preset {name}"))
}

#[test]
fn groups_have_their_factory_presets_and_all_round_trip() {
    let all = BrushPreset::defaults();
    let names = |g: &str| all.iter().filter(|p| p.group == g).map(|p| p.name.clone()).collect::<Vec<_>>();
    for n in ["Lápis", "Lapiseira", "Carvão", "Giz de cera", "Pastel", "Giz"] {
        assert!(names("lapis").contains(&n.to_string()), "{n}");
    }
    for n in ["Aerógrafo suave", "Spray", "Gotículas", "Ruído", "Spray colorido"] {
        assert!(names("aerografo").contains(&n.to_string()), "{n}");
    }
    for n in ["Borracha dura", "Borracha macia", "Borracha limpa-tipos", "Borracha áspera", "Borracha vetorial", "Borracha de várias camadas", "Borracha de encaixe", "Borracha dinâmica"] {
        assert!(names("borracha").contains(&n.to_string()), "{n}");
    }
    for p in BrushPreset::pencils().into_iter().chain(BrushPreset::airbrushes()).chain(BrushPreset::erasers()) {
        p.validate().unwrap_or_else(|e| panic!("{}: {e}", p.name));
        let back: BrushPreset = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p, "{}", p.name);
    }
    // a borracha de sempre da ferramenta continua a mesma
    let e = BrushPreset::eraser();
    assert_eq!((e.name.as_str(), e.mode, e.group.as_str()), ("Borracha", BrushMode::Erase, ""));
}

#[test]
fn factory_erasers_erase_a_black_layer() {
    for p in BrushPreset::erasers() {
        let mut img = Image::filled(200, 100, [0.0, 0.0, 0.0, 1.0]);
        let mut q = p.clone();
        q.size = 20.0;
        paint_stroke(&mut img, &q, &line(30.0, 170.0, 50.0, 2.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        let a = img.get(100, 50)[3];
        assert!(a < 0.5, "{} apagou o meio da linha: alfa {a}", p.name);
        assert!(img.get(100, 90)[3] > 0.99, "{} não apaga longe do traço", p.name);
    }
}

#[test]
fn fixed_paper_grain_leaves_the_same_holes_and_moving_grain_fills_them() {
    let mut p = BrushPreset::round("g", 20.0, 1.0);
    p.size_pressure_min = 1.0;
    p.antialias = Some(1);
    p.max_ink = true;
    p.grain = Some(Grain { kind: GrainKind::Speckle, scale: 1.0, depth: 1.0, contrast: 3.0, brightness: 0.0, subtract: true, moving: false, seed: 0 });
    let draw = |p: &BrushPreset, passes: usize| {
        let mut img = Image::new(200, 60);
        for k in 0..passes {
            paint_stroke(&mut img, p, &line(20.0, 180.0, 30.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1 + k as u64).unwrap();
        }
        (25..35).flat_map(|y| (60..140).map(move |x| (x, y))).map(|(x, y)| img.get(x, y)[3]).collect::<Vec<f32>>()
    };
    let one = draw(&p, 1);
    let holes = one.iter().filter(|a| **a < 0.2).count();
    assert!(holes > 40, "o grão do papel deixa falhas no meio: {holes}");
    let three = draw(&p, 3);
    let holes3 = three.iter().filter(|a| **a < 0.2).count();
    assert!(holes3 as f32 > holes as f32 * 0.8, "grão fixo: repassar não fecha os vales ({holes} -> {holes3})");
    let mut m = p.clone();
    m.max_ink = false;
    m.grain.as_mut().unwrap().moving = true;
    let moving = draw(&m, 1);
    let holes_m = moving.iter().filter(|a| **a < 0.2).count();
    assert!(holes_m < holes / 2, "grão que anda se acumula em nuvem: {holes_m} x {holes}");
}

#[test]
fn spray_scatters_particles_inside_the_tip_and_scales_with_size() {
    let mut p = BrushPreset::round("s", 60.0, 1.0);
    p.size_pressure_min = 1.0;
    p.antialias = Some(2);
    p.spray = Some(Spray { count: 10.0, size: 2.0, sync: false, size_min: 1.0, roundness_min: 1.0, bias: 0.0, count_exp: 2.0 });
    let ink = |size: f32| {
        let mut q = p.clone();
        q.size = size;
        let mut img = Image::new(400, 200);
        paint_stroke(&mut img, &q, &line(40.0, 360.0, 100.0, 2.0), [0.0, 0.0, 0.0, 1.0], 3).unwrap();
        let far = (0..400).map(|x| img.get(x, 100 - (size * 0.5) as u32 - 6)[3]).fold(0.0f32, f32::max);
        let near: f32 = (0..400).flat_map(|x| (0..200).map(move |y| (x, y))).map(|(x, y)| img.get(x, y)[3]).sum();
        (near, far)
    };
    let (i30, far30) = ink(30.0);
    let (i60, _) = ink(60.0);
    assert!(far30 < 1e-3, "nenhuma partícula fora do disco da ponta");
    // densidade igual na área: o dobro do tamanho cobre ~o dobro da tinta por comprimento
    let k = i60 / i30.max(1e-3);
    assert!((1.4..2.8).contains(&k), "tinta 60 px / 30 px = {k}");
}

#[test]
fn charcoal_stick_is_full_on_the_left_of_the_stroke() {
    let mut p = BrushPreset::round("c", 30.0, 1.0);
    p.size_pressure_min = 1.0;
    p.tip = BrushTip::Shape { shape: TipShape::Stick, aspect: 1.0 };
    p.follow_direction = true;
    p.max_ink = true;
    let mut img = Image::new(240, 80);
    paint_stroke(&mut img, &p, &line(30.0, 210.0, 40.0, 2.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    // indo para a direita, a esquerda do sentido é em cima
    let top = img.get(120, 30)[3];
    let bottom = img.get(120, 50)[3];
    assert!(top > 0.8 && bottom < top * 0.6, "cheio em cima ({top}), esmaecido embaixo ({bottom})");
}

#[test]
fn soft_tip_has_a_gaussian_tail_past_the_radius() {
    let soft = preset("Aerógrafo suave");
    let mut s = soft.clone();
    s.size = 30.0;
    let mut img = Image::new(240, 80);
    paint_stroke(&mut img, &s, &line(30.0, 210.0, 40.0, 2.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let col: Vec<f32> = (0..80).map(|y| img.get(120, y)[3]).collect();
    let peak = col.iter().cloned().fold(0.0f32, f32::max);
    assert!((0.5..0.7).contains(&peak), "pico ~0,6 como o aerógrafo suave do Clip Studio: {peak}");
    assert!(col[40 + 17] > 0.01, "cauda passa do raio de 15 px");
    assert!(col[40 + 25] < 0.005, "e acaba");
}
