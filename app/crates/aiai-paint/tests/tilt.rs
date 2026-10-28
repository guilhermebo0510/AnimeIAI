//! Inclinação da caneta no motor (auditoria 4.1d): a dinâmica de inclinação muda o tamanho, a ponta pode seguir o lado
//! em que a caneta deita, e sem inclinação (mouse) nada muda.

use aiai_paint::Image;
use aiai_paint::brush::{BrushPoint, BrushPreset, BrushTip, paint_stroke};
use aiai_paint::brush_dyn::{DynInput, Dynamics};

fn line(tilt: Option<[f32; 2]>) -> Vec<BrushPoint> {
    (0..=40).map(|i| BrushPoint { x: 20.0 + i as f32 * 4.0, y: 50.0, pressure: 1.0, tilt }).collect()
}

/// Altura da tinta (alfa >= 0,5) na coluna x = 100.
fn height_at_100(img: &Image) -> u32 {
    (0..img.height).filter(|&y| img.get(100, y)[3] >= 0.5).count() as u32
}

#[test]
fn tilt_dynamics_change_the_size_and_mouse_strokes_ignore_them() {
    let mut p = BrushPreset::round("inclinação", 20.0, 1.0);
    // em pé = 25% do tamanho, deitada (60°) = cheio
    p.size_dyn = Some(Dynamics { tilt: Some(DynInput::linear(0.25)), ..Default::default() });
    let draw = |t: Option<[f32; 2]>| {
        let mut img = Image::new(200, 100);
        paint_stroke(&mut img, &p, &line(t), [0.0, 0.0, 0.0, 1.0], 1);
        height_at_100(&img)
    };
    let (upright, lying, half, mouse) = (draw(Some([0.0, 0.0])), draw(Some([0.0, 60.0])), draw(Some([30.0, 0.0])), draw(None));
    assert!((4..=6).contains(&upright), "em pé: {upright} px");
    assert!((19..=21).contains(&lying), "deitada: {lying} px");
    assert!(half > upright + 4 && half < lying - 4, "meio deitada fica no meio: {half}");
    assert_eq!(mouse, lying, "sem inclinação a dinâmica não muda nada");
}

#[test]
fn the_tip_can_follow_the_side_the_pen_leans_to() {
    // ponta chata (30% de redondeza) que segue a inclinação; um carimbo só
    let mut p = BrushPreset::round("chata", 40.0, 1.0);
    p.tip = BrushTip::Round { hardness: 1.0, roundness: 0.3, angle: 0.0 };
    p.follow_tilt = true;
    let dab = |t: [f32; 2]| {
        let mut img = Image::new(100, 100);
        paint_stroke(&mut img, &p, &[BrushPoint { x: 50.0, y: 50.0, pressure: 1.0, tilt: Some(t) }], [0.0, 0.0, 0.0, 1.0], 1);
        let w = (0..100).filter(|&x| img.get(x, 50)[3] >= 0.5).count();
        let h = (0..100).filter(|&y| img.get(50, y)[3] >= 0.5).count();
        (w, h)
    };
    // deitada para a direita: o eixo comprido da ponta fica na horizontal; para baixo, na vertical
    let (w1, h1) = dab([45.0, 0.0]);
    let (w2, h2) = dab([0.0, 45.0]);
    assert!(w1 > 2 * h1, "deitada em X: {w1}x{h1}");
    assert!(h2 > 2 * w2, "deitada em Y: {w2}x{h2}");
    // em pé (inclinação < 1°) a ponta fica no ângulo dela
    let (w3, h3) = dab([0.0, 0.0]);
    assert_eq!((w3, h3), (w1, h1));
}
