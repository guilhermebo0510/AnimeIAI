//! Modelo de caneta do S25 (calibrado no Clip Studio): afinamento, velocidade, ponta chata, caneta de pixel, tinta pelo
//! máximo, suavização de serrilhado e os presets de fábrica.

use aiai_paint::Image;
use aiai_paint::brush::{BrushPoint, BrushPreset, StrokeBuffer, paint_stroke};
use aiai_paint::brush_dyn::{DynInput, Dynamics, Taper};

fn line(x0: f32, x1: f32, y: f32, step: f32) -> Vec<BrushPoint> {
    let n = ((x1 - x0) / step).round() as usize;
    (0..=n).map(|i| BrushPoint { x: x0 + (x1 - x0) * i as f32 / n as f32, y, pressure: 1.0, tilt: None }).collect()
}

/// Soma do alfa na coluna `x` (largura da linha horizontal ali).
fn width_at(img: &Image, x: u32) -> f32 {
    (0..img.height).map(|y| img.get(x, y)[3]).sum()
}

fn pen(name: &str) -> BrushPreset {
    BrushPreset::defaults().into_iter().find(|p| p.name == name).unwrap_or_else(|| panic!("preset {name}"))
}

#[test]
fn factory_pens_exist_validate_and_have_groups() {
    let all = BrushPreset::defaults();
    let groups = ["caneta", "marcador", "lapis", "pincel", "aerografo", "decoracao", "borracha", "mistura"];
    for p in &all {
        p.validate().unwrap_or_else(|e| panic!("{}: {e}", p.name));
        assert!(groups.contains(&p.group.as_str()), "{} sem grupo conhecido: {:?}", p.name, p.group);
    }
    for (name, group) in [
        ("Pena G", "caneta"),
        ("Pena G real", "caneta"),
        ("Pena de mapeamento", "caneta"),
        ("Pena nabo", "caneta"),
        ("Caligrafia", "caneta"),
        ("Caneta texturizada", "caneta"),
        ("Caneta rugosa", "caneta"),
        ("Caneta milimétrica", "marcador"),
        ("Caneta de feltro", "marcador"),
        ("Caneta de pixel", "marcador"),
        ("Marcador chato", "marcador"),
    ] {
        assert_eq!(pen(name).group, group, "{name}");
    }
    // todos desenham algo e voltam iguais do JSON
    for p in BrushPreset::pens() {
        let back: BrushPreset = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p, "{}", p.name);
        let mut img = Image::new(200, 80);
        paint_stroke(&mut img, &p, &line(30.0, 170.0, 40.0, 2.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
        assert!(img.data.iter().any(|q| q[3] > 0.5), "{} não desenhou", p.name);
    }
}

#[test]
fn old_presets_without_the_new_fields_still_load_and_paint_the_same() {
    // um pincel salvo antes do S25 (sem grupo, dinâmicas, afinamento...)
    let json = r#"{"name":"Meu","size":12,"tip":{"type":"round","hardness":0.9,"roundness":1.0,"angle":0.0},"spacing":0.1}"#;
    let p: BrushPreset = serde_json::from_str(json).unwrap();
    assert!(p.group.is_empty() && p.size_dyn.is_none() && p.taper.is_none() && p.antialias.is_none() && !p.max_ink);
    p.validate().unwrap();
    let mut q = BrushPreset::round("Meu", 12.0, 0.9);
    q.spacing = 0.1;
    let (mut a, mut b) = (Image::new(120, 60), Image::new(120, 60));
    paint_stroke(&mut a, &p, &line(20.0, 100.0, 30.0, 3.0), [0.0, 0.0, 0.0, 1.0], 5).unwrap();
    paint_stroke(&mut b, &q, &line(20.0, 100.0, 30.0, 3.0), [0.0, 0.0, 0.0, 1.0], 5).unwrap();
    assert_eq!(a.content_hash(), b.content_hash());
    // e não ganham nada no JSON
    let s = serde_json::to_string(&q).unwrap();
    for k in ["group", "antialias", "size_dyn", "taper", "stabilization", "edge", "max_ink", "velocity_full"] {
        assert!(!s.contains(&format!("\"{k}\"")), "{k} apareceu no JSON de um pincel antigo: {s}");
    }
}

#[test]
fn taper_thins_both_ends_and_the_live_preview_shows_the_end_at_the_cursor() {
    let mut p = BrushPreset::round("t", 16.0, 1.0);
    p.size_pressure_min = 1.0;
    p.antialias = Some(2);
    p.max_ink = true;
    p.taper = Some(Taper { start: 40.0, end: 40.0, min: 0.0, size: true, opacity: false, by_speed: false });
    let path = line(20.0, 300.0, 40.0, 1.0);
    let mut img = Image::new(320, 80);
    paint_stroke(&mut img, &p, &path, [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let mid = width_at(&img, 160);
    assert!(mid > 13.0, "corpo cheio: {mid}");
    assert!(width_at(&img, 24) < mid * 0.4, "entrada fina: {}", width_at(&img, 24));
    assert!(width_at(&img, 296) < mid * 0.4, "saída fina: {}", width_at(&img, 296));
    assert!(width_at(&img, 40) < width_at(&img, 55), "a entrada engrossa aos poucos");
    // aos pedaços dá o mesmo traço (os dabs da saída esperam o resto do caminho)
    let mut sb = StrokeBuffer::new(&p, [0.0, 0.0, 0.0, 1.0], 1, 320, 80, [0, 0]);
    for c in path.chunks(9) {
        sb.add(None, c);
    }
    // prévia no meio do traço: a cópia termina ali com a ponta afinada
    let mut preview = StrokeBuffer::new(&p, [0.0, 0.0, 0.0, 1.0], 1, 320, 80, [0, 0]);
    preview.add(None, &path[..150]);
    let mut tmp = preview.clone();
    tmp.finish(None);
    let mut pv = Image::new(320, 80);
    tmp.composite(&mut pv, [0, 0]);
    assert!(width_at(&pv, 166) < width_at(&pv, 120) * 0.5, "a prévia mostra a saída no cursor");
    sb.finish(None);
    let mut b = Image::new(320, 80);
    sb.composite(&mut b, [0, 0]);
    let d = img.data.iter().zip(&b.data).map(|(x, y)| (x[3] - y[3]).abs()).fold(0.0, f32::max);
    assert!(d < 1e-4, "pedaços = inteiro: {d}");
}

#[test]
fn velocity_thins_a_fast_stroke_only_with_time() {
    let mm = pen("Caneta milimétrica");
    let mut mm = mm;
    mm.size = 20.0;
    mm.stabilization = None;
    let draw = |step: f32, timed: bool| {
        let path = line(20.0, 380.0, 40.0, step);
        let times: Vec<f32> = (0..path.len()).map(|i| i as f32 * 0.008).collect();
        let mut sb = StrokeBuffer::new(&mm, [0.0, 0.0, 0.0, 1.0], 1, 400, 80, [0, 0]);
        if timed {
            sb.add_timed(None, &path, &times);
        } else {
            sb.add(None, &path);
        }
        sb.finish(None);
        let mut img = Image::new(400, 80);
        sb.composite(&mut img, [0, 0]);
        width_at(&img, 300)
    };
    let slow = draw(3.0, true);
    let fast = draw(15.0, true);
    let untimed = draw(15.0, false);
    assert!(fast < slow - 0.4, "rápido afina: lento {slow}, rápido {fast}");
    assert!((untimed - draw(3.0, false)).abs() < 0.05, "sem tempo, a velocidade conta como parada");
    // a curva da velocidade: em 100% vai ao mínimo
    let v = DynInput::slower_is_bigger(0.6);
    assert!((v.factor(1.0) - 0.6).abs() < 1e-6);
    let d = Dynamics { velocity: Some(v), ..Default::default() };
    assert!(d.base(1.0, 0.0) > d.base(1.0, 0.5));
}

#[test]
fn flat_nib_is_thin_one_way_and_wide_the_other() {
    let mut fm = pen("Marcador chato");
    fm.size = 30.0;
    let mut h = Image::new(200, 200);
    paint_stroke(&mut h, &fm, &line(40.0, 160.0, 100.0, 2.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let mut v = Image::new(200, 200);
    let vpath: Vec<BrushPoint> = line(40.0, 160.0, 100.0, 2.0).iter().map(|q| BrushPoint { x: q.y, y: q.x, pressure: 1.0, tilt: None }).collect();
    paint_stroke(&mut v, &fm, &vpath, [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let wh = width_at(&h, 100);
    let wv: f32 = (0..200).map(|x| v.get(x, 100)[3]).sum();
    assert!(wh < 11.0 && wv > 25.0, "horizontal fino {wh}, vertical largo {wv}");
    // a lateral fina da ponta pequena não some (no mínimo ~2 px de tinta)
    fm.size = 3.0;
    let mut s = Image::new(200, 60);
    paint_stroke(&mut s, &fm, &line(20.0, 180.0, 30.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    assert!(width_at(&s, 100) > 1.5, "ponta chata de 3 px aparece: {}", width_at(&s, 100));
}

#[test]
fn pixel_pen_draws_one_pixel_lines_without_doubled_steps() {
    let px = pen("Caneta de pixel");
    assert!(px.is_pixel_pen());
    let mut img = Image::new(200, 100);
    // diagonal rasa: um pixel por coluna, só 0 ou 1
    let path: Vec<BrushPoint> = (0..=60).map(|i| BrushPoint { x: 20.0 + i as f32 * 2.5, y: 20.0 + i as f32 * 1.0, pressure: 1.0, tilt: None }).collect();
    paint_stroke(&mut img, &px, &path, [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    assert!(img.data.iter().all(|q| q[3] == 0.0 || q[3] == 1.0), "sem antisserrilhado");
    for x in 25..165 {
        let n = (0..100).filter(|y| img.get(x, *y)[3] > 0.5).count();
        assert_eq!(n, 1, "coluna {x}: {n} pixels");
    }
    // diagonal de 45°: nenhum pixel com vizinho em L (degrau dobrado)
    let mut d = Image::new(100, 100);
    let path: Vec<BrushPoint> = (0..=40).map(|i| BrushPoint { x: 10.3 + i as f32 * 1.7, y: 10.6 + i as f32 * 1.7, pressure: 1.0, tilt: None }).collect();
    paint_stroke(&mut d, &px, &path, [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let on = |x: i32, y: i32| x >= 0 && y >= 0 && x < 100 && y < 100 && d.get(x as u32, y as u32)[3] > 0.5;
    for y in 1..99 {
        for x in 1..99 {
            if on(x, y) && on(x + 1, y) && on(x, y + 1) {
                panic!("degrau em L em ({x}, {y})");
            }
        }
    }
}

#[test]
fn max_ink_keeps_the_tip_profile_and_antialias_levels_differ() {
    let mut p = BrushPreset::round("m", 10.0, 1.0);
    p.size_pressure_min = 1.0;
    p.antialias = Some(2);
    p.spacing = 0.05;
    p.max_ink = true;
    let mut a = Image::new(200, 60);
    paint_stroke(&mut a, &p, &line(20.0, 180.0, 30.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    let mut over = p.clone();
    over.max_ink = false;
    let mut b = Image::new(200, 60);
    paint_stroke(&mut b, &over, &line(20.0, 180.0, 30.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    // a borda da linha (linha 25 = 4,5 px do eixo) fica como a de UM carimbo; somando, ela escurece
    let edge_max = a.get(100, 25)[3];
    let edge_over = b.get(100, 25)[3];
    assert!(edge_max < 0.6 && edge_over > edge_max + 0.2, "máximo {edge_max}, soma {edge_over}");
    // nível 0: só 0 ou 1; nível 2: tem meio-tom na borda
    let mut hard = p.clone();
    hard.antialias = Some(0);
    let mut c = Image::new(200, 60);
    paint_stroke(&mut c, &hard, &line(20.0, 180.0, 30.0, 1.0), [0.0, 0.0, 0.0, 1.0], 1).unwrap();
    assert!(c.data.iter().all(|q| q[3] == 0.0 || q[3] > 0.999), "sem suavização: borda dura");
    assert!(a.data.iter().any(|q| q[3] > 0.2 && q[3] < 0.8), "suavização média: meio-tom");
    // a G de 10 px: corpo ≈ 8,9 px como no Clip Studio
    let w = width_at(&a, 100);
    assert!((w - 8.9).abs() < 0.4, "corpo da linha de 10 px: {w}");
}
