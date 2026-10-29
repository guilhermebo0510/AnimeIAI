//! Copiloto de traço: as partes que não dependem do gerador (janela de contexto, corte na caixa).
use aiai_tools::copilot::{clip_to_box, context_window};
use aiai_tools::vectorize::InkPath;

#[test]
fn context_window_is_square_at_least_384_and_inside_the_canvas() {
    // caixa pequena: a janela cresce até 384 em volta dela
    let w = context_window([100.0, 100.0, 50.0, 50.0], 2.0, (1920, 1080));
    assert_eq!((w[2], w[3]), (384, 384));
    assert!(w[0] as f32 <= 100.0 && (w[0] + w[2]) as f32 >= 150.0);
    // perto da borda: empurrada para dentro
    let w = context_window([1900.0, 1060.0, 10.0, 10.0], 2.0, (1920, 1080));
    assert!(w[0] + w[2] <= 1920 && w[1] + w[3] <= 1080);
    // caixa grande: margem ×2, limitada ao canvas
    let w = context_window([500.0, 200.0, 400.0, 300.0], 2.0, (1920, 1080));
    assert_eq!(w[2], 800);
}

#[test]
fn a_stroke_crossing_the_box_is_cut_at_its_edge() {
    // reta horizontal de x = 0 a 200 (imagem), escala 1, deslocada 100 no canvas → 100..300; caixa x 150..250
    let p = InkPath { points: vec![[0.0, 50.0], [200.0, 50.0]], widths: vec![3.0, 3.0], closed: false };
    let out = clip_to_box(&p, 1.0, [100.0, 0.0], [150.0, 0.0, 250.0, 100.0]);
    assert_eq!(out.len(), 1);
    let (a, b) = (out[0].points[0], *out[0].points.last().unwrap());
    assert!((a[0] + 100.0 - 150.0).abs() <= 2.5 && (b[0] + 100.0 - 250.0).abs() <= 2.5, "{a:?} {b:?}");
    // entra e sai duas vezes: dois pedaços
    let z = InkPath { points: vec![[0.0, 10.0], [100.0, 10.0], [100.0, 90.0], [0.0, 90.0]], widths: vec![2.0; 4], closed: false };
    let out = clip_to_box(&z, 1.0, [0.0, 0.0], [50.0, 0.0, 200.0, 200.0]);
    assert_eq!(out.len(), 1, "o U inteiro dentro de x ≥ 50 é um pedaço só");
    let out = clip_to_box(&z, 1.0, [0.0, 0.0], [50.0, 0.0, 200.0, 50.0]);
    assert_eq!(out.len(), 1, "só o braço de cima");
    // tudo dentro: igual, e o fechado continua fechado
    let c = InkPath { points: vec![[10.0, 10.0], [40.0, 10.0], [25.0, 40.0]], widths: vec![2.0; 3], closed: true };
    assert_eq!(clip_to_box(&c, 1.0, [0.0, 0.0], [0.0, 0.0, 100.0, 100.0]), vec![c.clone()]);
    // tudo fora: nada
    assert!(clip_to_box(&c, 1.0, [500.0, 500.0], [0.0, 0.0, 100.0, 100.0]).is_empty());
}
