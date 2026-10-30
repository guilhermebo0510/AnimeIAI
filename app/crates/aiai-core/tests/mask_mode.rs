//! Modos da camada-máscara (fase 3, C2): os quatro track mattes do After Effects.
use aiai_core::{LayerData, LayerType, MaskMode};

#[test]
fn the_four_mask_modes_read_alpha_or_luma() {
    let white_half = [0.5, 0.5, 0.5, 0.5]; // branco com alfa 0,5 (pré-multiplicado)
    let black = [0.0, 0.0, 0.0, 1.0];
    let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
    assert!(near(MaskMode::Alpha.value(white_half), 0.5));
    assert!(near(MaskMode::Luma.value(white_half), 0.5));
    assert!(near(MaskMode::AlphaInverted.value(white_half), 0.5));
    assert!(near(MaskMode::LumaInverted.value(white_half), 0.5));
    assert!(near(MaskMode::Alpha.value(black), 1.0));
    assert!(near(MaskMode::Luma.value(black), 0.0));
    assert!(near(MaskMode::AlphaInverted.value(black), 0.0));
    assert!(near(MaskMode::LumaInverted.value(black), 1.0));
    assert!(near(MaskMode::Luma.value([0.0; 4]), 0.0), "transparente não mostra");
}

#[test]
fn old_projects_keep_luma_and_the_default_is_not_written() {
    let l = LayerData::new(LayerType::Raster);
    assert_eq!(l.mask_mode, MaskMode::Luma);
    let v = serde_json::to_value(&l).unwrap();
    assert!(v.get("mask_mode").is_none(), "o padrão não vai para o arquivo");
    let mut v2 = v.clone();
    v2["mask_mode"] = serde_json::json!("alpha_inverted");
    let l2: LayerData = serde_json::from_value(v2).unwrap();
    assert_eq!(l2.mask_mode, MaskMode::AlphaInverted);
}
