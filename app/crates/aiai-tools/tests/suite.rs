//! Fluxo completo de ferramenta sem UI: desenhar linha por curvas → balde com paleta → lint → render.

use aiai_core::*;
use aiai_render::{RenderOptions, render_frame};
use aiai_tools::draw::{CurveSpec, DrawRequest, draw_command};
use aiai_tools::fill::{FillRequest, FillSeed, fill_command, lint};
use serde_json::json;

fn create(s: &mut CoreSession, parent: Option<&Id>, name: &str, data: NodeKind) -> Id {
    s.apply(Command::CreateNode { id: None, parent: parent.cloned(), index: None, name: name.into(), data }).unwrap().created[0].clone()
}

fn curve(points: &[[f32; 3]], width: f32, color: &ColorRef, closed: bool) -> CurveSpec {
    serde_json::from_value(json!({
        "points": points.iter().map(|p| p.to_vec()).collect::<Vec<_>>(),
        "width": width, "tip": "g_pen", "color": color, "closed": closed
    }))
    .unwrap()
}

#[test]
fn draw_fill_lint_render() {
    let mut s = CoreSession::for_tests("tools");
    s.apply(Command::SetProjectProps { patch: json!({"settings": {"width": 640, "height": 360}}) }).unwrap();
    let ep = create(&mut s, None, "EP", NodeKind::Episode(EpisodeData::default()));
    let sc = create(&mut s, Some(&ep), "S", NodeKind::Scene(SceneData::default()));
    let cut = create(&mut s, Some(&sc), "C001", NodeKind::Cut(CutData::new(24)));
    let pal = create(
        &mut s,
        None,
        "Settei",
        NodeKind::Palette(PaletteData {
            colors: [("linha", "#40302c"), ("pele", "#fbe3d3"), ("cabelo", "#f2d36b"), ("olho", "#3d6fd1"), ("branco", "#ffffff")]
                .iter()
                .map(|(id, hex)| NamedColor { id: Id::from(*id), name: id.to_string(), group: id.to_string(), role: "normal".into(), rgba: Rgba::from_hex(hex).unwrap(), variants: Default::default() })
                .collect(),
            active_variant: String::new(),
        }),
    );
    let c = |id: &str| ColorRef::Palette { palette: pal.clone(), color: Id::from(id) };
    let fill_layer = create(&mut s, Some(&cut), "A-cor", NodeKind::Layer(LayerData::new(LayerType::Fill)));
    let line_layer = create(&mut s, Some(&cut), "A-linha", NodeKind::Layer(LayerData::new(LayerType::Vector)));
    create(&mut s, Some(&fill_layer), "A-cor1", NodeKind::Cel(CelData::new(1)));
    create(&mut s, Some(&line_layer), "A-linha1", NodeKind::Cel(CelData::new(1)));
    s.apply(Command::SetProps { id: fill_layer.clone(), name: None, data: json!({"line_refs": [line_layer.0]}) }).unwrap();

    // Rosto simples: contorno, franja (com um buraco de ~2 px proposital), dois olhos (simetria).
    let face = curve(&[[250.0, 120.0, 0.8], [240.0, 200.0, 1.0], [320.0, 280.0, 1.0], [400.0, 200.0, 1.0], [390.0, 120.0, 0.8]], 4.0, &c("linha"), false);
    let hair = curve(&[[248.0, 128.0, 0.6], [270.0, 70.0, 1.0], [320.0, 55.0, 1.0], [370.0, 70.0, 1.0], [393.0, 128.0, 0.6]], 4.0, &c("linha"), false);
    let fringe = curve(&[[250.0, 125.0, 1.0], [285.0, 150.0, 1.0], [320.0, 128.0, 1.0], [355.0, 150.0, 1.0], [388.0, 124.0, 1.0]], 3.0, &c("linha"), false);
    let eye = curve(&[[285.0, 185.0, 1.0], [300.0, 175.0, 1.0], [300.0, 205.0, 1.0], [285.0, 200.0, 1.0]], 3.0, &c("linha"), true);
    let mut draw = DrawRequest { cut: cut.clone(), frame: 0, layer: line_layer.clone(), curves: vec![face, hair, fringe], mirror_x: None };
    s.apply(draw_command(&s.project, &draw).unwrap()).unwrap();
    draw.curves = vec![eye];
    draw.mirror_x = Some(320.0);
    s.apply(draw_command(&s.project, &draw).unwrap()).unwrap();

    // Antes de pintar: o lint aponta regiões fechadas sem cor.
    let before = lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap();
    assert!(before.unpainted.len() >= 4, "rosto, franja e dois olhos: {:?}", before.unpainted.len());

    // Balde em cada região (sementes do próprio lint, como o agente faria).
    let seeds: Vec<FillSeed> = before
        .unpainted
        .iter()
        .map(|r| {
            let (x, y) = (r.point[0] as f32, r.point[1] as f32);
            let color = if y < 125.0 { c("cabelo") } else if r.area < 600 { c("olho") } else { c("pele") };
            FillSeed { x, y, color }
        })
        .collect();
    let req = FillRequest { cut: cut.clone(), frame: 0, layer: fill_layer.clone(), seeds, line_layers: vec![], gap: 3.0, expand: 2, line_threshold: 0.35 };
    let cmd = fill_command(&s.project, &mut s.blobs, &req).unwrap();
    s.apply(cmd).unwrap();
    let after = lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap();
    assert!(after.unpainted.is_empty(), "tudo pintado: {:?}", after.unpainted);

    let out = render_frame(&s.project, &s.blobs, &cut, 0, &RenderOptions { background: Some([1.0, 1.0, 1.0]), ..Default::default() }).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    if let Ok(dir) = std::env::var("AIAI_EVIDENCE_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        out.image.save_png(&std::path::Path::new(&dir).join("rosto_linha_balde.png")).unwrap();
    }
    // a pele dentro do rosto tem a cor do settei, sem halo branco junto da linha
    let p = out.image.get(320, 240);
    assert!((p[0] - 0xfb as f32 / 255.0).abs() < 0.02 && (p[2] - 0xd3 as f32 / 255.0).abs() < 0.02, "pele: {p:?}");
    // undo do balde volta a ter regiões sem cor
    s.undo().unwrap();
    assert!(!lint(&s.project, &s.blobs, &cut, 0, &fill_layer, 3.0, 30).unwrap().unpainted.is_empty());
}
