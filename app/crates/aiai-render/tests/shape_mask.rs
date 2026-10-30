//! Máscaras desenhadas (fase 3, C2.2): cobertura, borda suave, expansão e combinação.
use aiai_core::{MaskPoint, ShapeMask, ShapeMaskMode};
use aiai_paint::Affine;
use aiai_render::shape_mask::coverage;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32, mode: ShapeMaskMode) -> ShapeMask {
    let p = |x, y| MaskPoint { pos: [x, y], h_in: [0.0, 0.0], h_out: [0.0, 0.0] };
    ShapeMask { points: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)], mode, feather: 0.0, expansion: 0.0, opacity: 1.0, inverted: false }
}

const ID: Affine = Affine([1.0, 0.0, 0.0, 0.0, 1.0, 0.0]);

#[test]
fn a_rectangle_shows_inside_and_hides_outside() {
    let c = coverage(&[rect(20.0, 20.0, 80.0, 80.0, ShapeMaskMode::Add)], &ID, 100, 100).unwrap();
    assert!(c[50 * 100 + 50] > 0.99);
    assert!(c[5 * 100 + 5] < 0.01);
    assert!(coverage(&[], &ID, 100, 100).is_none(), "sem máscara não há cobertura");
}

#[test]
fn feather_makes_a_monotonic_ramp_across_the_edge() {
    let mut m = rect(0.0, 0.0, 50.0, 100.0, ShapeMaskMode::Add);
    m.feather = 10.0;
    let c = coverage(&[m], &ID, 100, 100).unwrap();
    let row: Vec<f32> = (30..70).map(|x| c[50 * 100 + x]).collect();
    assert!(row.windows(2).all(|w| w[1] <= w[0] + 1e-4), "cai sem voltar: {row:?}");
    assert!(c[50 * 100 + 30] > 0.9 && c[50 * 100 + 70] < 0.1, "a rampa fica perto da borda");
    assert!((c[50 * 100 + 50] - 0.5).abs() < 0.15, "meio da rampa na borda: {}", c[50 * 100 + 50]);
}

#[test]
fn negative_expansion_shrinks_the_shape() {
    let mut m = rect(20.0, 20.0, 80.0, 80.0, ShapeMaskMode::Add);
    m.expansion = -5.0;
    let c = coverage(&[m], &ID, 100, 100).unwrap();
    assert!(c[50 * 100 + 22] < 0.01, "a borda entrou 5 px");
    assert!(c[50 * 100 + 30] > 0.99);
    let mut g = rect(20.0, 20.0, 80.0, 80.0, ShapeMaskMode::Add);
    g.expansion = 5.0;
    let c = coverage(&[g], &ID, 100, 100).unwrap();
    assert!(c[50 * 100 + 17] > 0.99 && c[50 * 100 + 10] < 0.01, "e cresce 5 px");
}

#[test]
fn subtract_after_add_makes_a_hole_and_the_layer_transform_moves_the_mask() {
    let c = coverage(&[rect(10.0, 10.0, 90.0, 90.0, ShapeMaskMode::Add), rect(40.0, 40.0, 60.0, 60.0, ShapeMaskMode::Subtract)], &ID, 100, 100).unwrap();
    assert!(c[50 * 100 + 50] < 0.01 && c[20 * 100 + 20] > 0.99);
    // a camada andou +30 em x: a máscara vai junto
    let moved = Affine([1.0, 0.0, 30.0, 0.0, 1.0, 0.0]);
    let c = coverage(&[rect(0.0, 0.0, 20.0, 100.0, ShapeMaskMode::Add)], &moved, 100, 100).unwrap();
    assert!(c[50 * 100 + 10] < 0.01 && c[50 * 100 + 40] > 0.99);
}
