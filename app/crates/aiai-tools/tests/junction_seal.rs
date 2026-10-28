//! Teste de vedação de junções (auditoria 4.9h, teste 3 da bancada 4.9.6): dois traços abertos de caneta de verdade
//! (Pena G, com a pressão caindo na ponta como na mão) se encontram em V de 15°, em L, em T e em linha reta (180°), e o
//! balde do app (fechamento de buracos padrão de 3 px) não pode vazar de uma região para a outra. Sem camada de
//! "Fecho do balde".

use aiai_paint::Image;
use aiai_paint::brush::{BrushPreset, StrokeBuffer};
use aiai_paint::fill::{FillParams, label_regions, sealed_line_boundary};
use aiai_tools::paint::{PaintStrokeSpec, brush_paths_timed};

const W: u32 = 400;
const H: u32 = 300;

fn pen(size: f32) -> BrushPreset {
    let mut p = BrushPreset::defaults().into_iter().find(|p| p.name == "Pena G").expect("Pena G");
    p.size = size;
    p
}

/// Traço reto de `a` a `b`, um ponto a cada 3 px; a pressão cai para `pa` / `pb` nos últimos 25 px de cada ponta
/// (1 = sem afinar).
fn line(a: [f32; 2], b: [f32; 2], pa: f32, pb: f32) -> Vec<[f32; 3]> {
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    let n = (len / 3.0).ceil().max(1.0) as usize;
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let s = t * len;
            let ramp = |d: f32, p0: f32| {
                let u = (d / 25.0).clamp(0.0, 1.0);
                p0 + (1.0 - p0) * u * u * (3.0 - 2.0 * u)
            };
            let p = ramp(s, pa).min(ramp(len - s, pb));
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, p]
        })
        .collect()
}

fn draw(strokes: &[Vec<[f32; 3]>], preset: &BrushPreset) -> Image {
    let mut img = Image::new(W, H);
    for (k, st) in strokes.iter().enumerate() {
        let spec = PaintStrokeSpec { points: st.iter().map(|p| p.to_vec()).collect(), smooth: false };
        for (path, _) in brush_paths_timed(std::slice::from_ref(&spec)).expect("caminho") {
            let mut sb = StrokeBuffer::new(preset, [0.0, 0.0, 0.0, 1.0], 1 + k as u64, W, H, [0, 0]);
            let layer = img.clone();
            sb.add(Some(&layer), &path);
            sb.finish(Some(&layer));
            sb.composite(&mut img, [0, 0]);
        }
    }
    img
}

/// Rótulos das sementes com o balde do app (linha a partir de 35% de alfa, fechamento de 3 px, solda de 12 px; a mesma
/// fronteira de `aiai_tools::fill::compute_regions`).
fn labels(img: &Image, seeds: &[(u32, u32)]) -> Vec<u32> {
    let b = sealed_line_boundary(img, 0.35, 12.0);
    let r = label_regions(&b, W as usize, H as usize, &FillParams { gap: 3.0, expand: 0, line_threshold: 0.35 });
    seeds.iter().map(|&(x, y)| r.label_at(x, y)).collect()
}

fn assert_sealed(name: &str, img: &Image, seeds: &[(u32, u32)]) {
    let l = labels(img, seeds);
    for (i, a) in l.iter().enumerate() {
        assert!(*a != 0, "{name}: semente {:?} caiu na linha", seeds[i]);
        for (j, b) in l.iter().enumerate().skip(i + 1) {
            if a == b {
                let _ = img.save_png(std::path::Path::new(&std::env::temp_dir().join(format!("vedacao_{name}.png"))));
            }
            assert!(a != b, "{name}: o balde vaza entre {:?} e {:?} (rótulo {a})", seeds[i], seeds[j]);
        }
    }
}

/// Cada caso nos tamanhos de linha mais usados e com a ponta afinando até 40% e até 25% da pressão (com a Pena G, abaixo
/// de ~35% de pressão a linha de 3 px já quase some: a 10% o buraco no canto passa de 17 px e é um canto aberto de fato).
fn sizes_and_tapers() -> Vec<(f32, f32)> {
    let mut v = Vec::new();
    for size in [3.0, 5.0, 8.0] {
        for taper in [0.4, 0.25] {
            v.push((size, taper));
        }
    }
    v
}

#[test]
fn v_junction_at_15_degrees_holds_the_fill() {
    for (size, p) in sizes_and_tapers() {
        let tip = [60.0f32, 200.0];
        let ang = 15f32.to_radians();
        let a = [tip[0] + 300.0, tip[1]];
        let b = [tip[0] + 300.0 * ang.cos(), tip[1] - 300.0 * ang.sin()];
        // as duas linhas da mecha começam afinadas na ponta do V e passam um pouco da linha que fecha
        let strokes = vec![
            line(tip, [a[0] + 6.0, a[1]], p, 1.0),
            line(tip, [b[0] + 6.0 * ang.cos(), b[1] - 6.0 * ang.sin()], p, 1.0),
            line([a[0], a[1] + 8.0], [b[0], b[1] - 8.0], 1.0, 1.0),
        ];
        let img = draw(&strokes, &pen(size));
        // dentro do V (perto da boca, onde é largo) e fora (acima, abaixo e à esquerda da ponta)
        assert_sealed(&format!("V_{size}_{p}"), &img, &[(300, 185), (5, 5)]);
    }
}

#[test]
fn l_junction_holds_the_fill() {
    for (size, p) in sizes_and_tapers() {
        let c = [80.0f32, 250.0];
        let strokes = vec![
            line(c, [330.0, 250.0], p, 1.0),
            line(c, [80.0, 40.0], p, 1.0),
            line([324.0, 256.0], [324.0, 44.0], 1.0, 1.0),
            line([74.0, 50.0], [330.0, 50.0], 1.0, 1.0),
        ];
        let img = draw(&strokes, &pen(size));
        assert_sealed(&format!("L_{size}_{p}"), &img, &[(200, 150), (40, 290)]);
    }
}

#[test]
fn t_junction_holds_the_fill() {
    for (size, p) in sizes_and_tapers() {
        // retângulo de traços cheios e uma divisória que chega afinada na base (o pé do T) e no topo
        let strokes = vec![
            line([50.0, 40.0], [350.0, 40.0], 1.0, 1.0),
            line([50.0, 260.0], [350.0, 260.0], 1.0, 1.0),
            line([50.0, 34.0], [50.0, 266.0], 1.0, 1.0),
            line([350.0, 34.0], [350.0, 266.0], 1.0, 1.0),
            line([200.0, 40.0], [200.0, 260.0], p, p),
        ];
        let img = draw(&strokes, &pen(size));
        assert_sealed(&format!("T_{size}_{p}"), &img, &[(120, 150), (280, 150), (5, 5)]);
    }
}

#[test]
fn straight_joint_at_180_degrees_holds_the_fill() {
    for (size, p) in sizes_and_tapers() {
        // a base é feita de dois traços que se encontram de ponta a ponta no meio, os dois afinando ali
        let strokes = vec![
            line([50.0, 40.0], [350.0, 40.0], 1.0, 1.0),
            line([50.0, 34.0], [50.0, 266.0], 1.0, 1.0),
            line([350.0, 34.0], [350.0, 266.0], 1.0, 1.0),
            line([44.0, 260.0], [200.0, 260.0], 1.0, p),
            line([200.0, 260.0], [356.0, 260.0], p, 1.0),
        ];
        let img = draw(&strokes, &pen(size));
        assert_sealed(&format!("reta_{size}_{p}"), &img, &[(200, 150), (200, 290)]);
    }
}



#[test]
fn without_sealing_the_tapered_corner_leaks() {
    // controle: só o limiar de alfa (o balde antes da 4.9h) vaza no L com as pontas afinando a 25%
    let c = [80.0f32, 250.0];
    let strokes = vec![
        line(c, [330.0, 250.0], 0.25, 1.0),
        line(c, [80.0, 40.0], 0.25, 1.0),
        line([324.0, 256.0], [324.0, 44.0], 1.0, 1.0),
        line([74.0, 50.0], [330.0, 50.0], 1.0, 1.0),
    ];
    let img = draw(&strokes, &pen(3.0));
    let b = aiai_paint::fill::boundary_from_alpha(&img, 0.35);
    let r = label_regions(&b, W as usize, H as usize, &FillParams { gap: 3.0, expand: 0, line_threshold: 0.35 });
    assert_eq!(r.label_at(200, 150), r.label_at(40, 290), "o controle devia vazar");
}
