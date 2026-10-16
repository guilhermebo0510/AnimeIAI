//! Corte de gag: fundo de faixas radiais, linhas de foco, onomatopeia "ゴゴゴ" vertical com contorno, e pétalas
//! (partículas) caindo por cima.

use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::fx2d::{GagBgRequest, GagPattern, LinesKind, LinesRequest, TextRequest, gag_bg_command, lines_command, text_command};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn layer(s: &mut CoreSession, cut: &Id, name: &str, kind: LayerType) -> Id {
    let l = create(s, Some(cut), name, NodeKind::Layer(LayerData::new(kind)));
    create(s, Some(&l), "1", NodeKind::Cel(CelData::new(1)));
    l
}

#[test]
fn gag_cut_with_text_lines_and_particles() {
    let (w, h) = (960u32, 540u32);
    let mut s = CoreSession::for_tests("gag");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": w, "height": h}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C", NodeKind::Cut(CutData::new(48)));
    let bg = layer(&mut s, &cut, "BG-gag", LayerType::Raster);
    let lines = layer(&mut s, &cut, "FX-linhas", LayerType::Vector);
    let txt_fill = layer(&mut s, &cut, "FX-texto", LayerType::Raster);
    let txt_line = layer(&mut s, &cut, "FX-texto-contorno", LayerType::Vector);
    let parts = create(&mut s, Some(&cut), "FX-petalas", NodeKind::Layer(LayerData::new(LayerType::Particles)));
    let sys = aiai_anim::particles::preset("petalas", w as f32, h as f32, 5).unwrap();
    s.apply(Command::SetProps { id: parts, name: None, data: json!({"params": {"particles": sys}}) }).unwrap();

    let c = gag_bg_command(&s.project, &mut s.blobs, &GagBgRequest { cut: cut.clone(), frame: 0, layer: bg, pattern: GagPattern::RadialStripes { center: [480.0, 270.0], count: 36, colors: ["#5b3fa0".into(), "#3a2470".into()] } }).unwrap();
    s.apply(c).unwrap();
    let lr = LinesRequest { cut: cut.clone(), frame: 0, layer: lines, kind: LinesKind::Focus { center: [480.0, 270.0], clear: [260.0, 170.0] }, count: 160, length: [300.0, 700.0], width: [2.0, 7.0], color: ColorRef::Direct { rgba: Rgba::from_hex("#ffffffcc").unwrap() }, seed: 1 };
    s.apply(lines_command(&s.project, &lr).unwrap()).unwrap();
    let tr = TextRequest {
        cut: cut.clone(), frame: 0, fill_layer: txt_fill, outline_layer: Some(txt_line), text: "ゴゴゴ".into(), font: None, size: 120.0,
        center: [800.0, 270.0], vertical: true, letter_spacing: -10.0, rotation: 8.0, skew: 0.15, scale: [1.2, 1.0],
        color: ColorRef::Direct { rgba: Rgba::from_hex("#e8213a").unwrap() }, outline_width: 6.0, outline_color: Some(ColorRef::Direct { rgba: Rgba::from_hex("#1a0f14").unwrap() }),
    };
    let c = text_command(&s.project, &mut s.blobs, &tr).unwrap();
    s.apply(c).unwrap();
    let out = render_frame(&s.project, &s.blobs, &cut, 24, &RenderOptions::default()).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        out.image.save_png(&std::path::Path::new(&dir).join("corte_gag.png")).unwrap();
    }
    let reds = out.image.data.iter().filter(|p| p[0] > 0.8 && p[1] < 0.3 && p[2] < 0.35).count();
    assert!(reds > 2000, "onomatopeia vermelha desenhada: {reds}");
    let f30 = render_frame(&s.project, &s.blobs, &cut, 30, &RenderOptions::default()).unwrap().image;
    assert_ne!(out.image.content_hash(), f30.content_hash(), "pétalas animam");
}
